//! Shared HTTP plumbing for the cloud providers.
//!
//! * The request runs on a helper thread and the caller polls `abort` every 100 ms, so
//!   cancelling does not wait for the provider (a hung call is abandoned, not awaited).
//! * Error text never contains the URL's query, headers or the response body of an auth
//!   failure: keys travel in headers only, and reqwest errors are stripped with `without_url`.
//! * Redirects are disabled by every client built here: they could carry the key elsewhere.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder};
use serde_json::Value;
use vad_core::VadError;

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const ABORT_POLL: Duration = Duration::from_millis(100);

pub(crate) fn client(timeout: Duration) -> Result<Client, VadError> {
    Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| VadError::Llm(format!("cliente HTTP: {e}")))
}

/// Retry policy for transient failures (429 and 5xx). Connection failures are **not**
/// retried: they mean "offline" and go straight to the disclosed fallback (§4.21).
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Extra attempts after the first.
    pub max_retries: u32,
    /// First backoff; doubles each retry, with ±50 % jitter.
    pub base: Duration,
    /// Upper bound for one wait (also applied to `Retry-After`).
    pub cap: Duration,
}

impl RetryPolicy {
    pub const DEFAULT: Self = Self { max_retries: 2, base: Duration::from_secs(1), cap: Duration::from_secs(30) };
    /// Single attempt, for "Testar ligação": a 429 there is a result to show, not to hide.
    pub const NONE: Self = Self { max_retries: 0, base: Duration::ZERO, cap: Duration::ZERO };

    fn delay(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        let exp = self.base.saturating_mul(1u32 << attempt.min(8));
        let jitter = 0.5 + jitter_unit(); // 0.5 .. 1.5
        let own = exp.mul_f64(jitter);
        retry_after.map_or(own, |ra| ra.max(own)).min(self.cap)
    }
}

/// Cheap [0,1) pseudo-random value from the clock; jitter does not need a CSPRNG.
fn jitter_unit() -> f64 {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    f64::from(n.wrapping_mul(2_654_435_761) % 1000) / 1000.0
}

struct Failure {
    error: VadError,
    retryable: bool,
    retry_after: Option<Duration>,
}

impl Failure {
    fn fatal(error: VadError) -> Self {
        Self { error, retryable: false, retry_after: None }
    }
}

fn run(req: RequestBuilder) -> Result<Value, Failure> {
    let resp = req.send().map_err(|e| {
        Failure::fatal(if e.is_timeout() {
            VadError::LlmTimeout("o provider não respondeu a tempo".into())
        } else if e.is_connect() {
            VadError::NoNetwork(format!("sem ligação ao provider (offline ou endereço errado): {}", e.without_url()))
        } else {
            VadError::Llm(format!("falha de ligação ao provider: {}", e.without_url()))
        })
    })?;
    let status = resp.status();
    if !status.is_success() {
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        // Bodies are not echoed on auth errors: some providers repeat part of the key.
        let (error, retryable) = match status.as_u16() {
            401 | 403 => (VadError::LlmAuthFailed(format!("HTTP {status}")), false),
            429 => (VadError::LlmRateLimited("HTTP 429".into()), true),
            404 => (VadError::Llm("HTTP 404: endpoint ou modelo não encontrado — verifica o endereço e o nome do modelo".into()), false),
            code => {
                let body = resp.text().unwrap_or_default();
                (VadError::Llm(format!("HTTP {status}: {}", body.chars().take(200).collect::<String>())), code >= 500)
            }
        };
        return Err(Failure { error, retryable, retry_after });
    }
    resp.json()
        .map_err(|e| Failure::fatal(VadError::Llm(format!("resposta do provider não é JSON válido: {e}"))))
}

/// Waits `d` in 100 ms slices so a cancel is honoured during a backoff too.
fn sleep_abortable(d: Duration, abort: Option<&AtomicBool>) -> Result<(), VadError> {
    let end = std::time::Instant::now() + d;
    while std::time::Instant::now() < end {
        if abort.is_some_and(|a| a.load(Ordering::Relaxed)) {
            return Err(VadError::LlmCancelled);
        }
        std::thread::sleep(ABORT_POLL.min(end.saturating_duration_since(std::time::Instant::now())));
    }
    Ok(())
}

fn attempt(req: RequestBuilder, abort: Option<&AtomicBool>) -> Result<Value, Failure> {
    let cancelled = || abort.is_some_and(|a| a.load(Ordering::Relaxed));
    if cancelled() {
        return Err(Failure::fatal(VadError::LlmCancelled));
    }
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send(run(req));
    });
    loop {
        match rx.recv_timeout(ABORT_POLL) {
            Ok(res) => return res,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if cancelled() {
                    return Err(Failure::fatal(VadError::LlmCancelled));
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err(Failure::fatal(VadError::Llm("o pedido ao provider terminou inesperadamente".into())));
            }
        }
    }
}

/// Sends `req` and returns the JSON body, honouring `abort` while waiting and retrying 429/5xx
/// with backoff+jitter (and the provider's `Retry-After`) per `policy`.
pub(crate) fn execute(
    req: RequestBuilder,
    abort: Option<&AtomicBool>,
    policy: RetryPolicy,
    secret: Option<&str>,
) -> Result<Value, VadError> {
    let mut tries = 0;
    let mut req = req;
    loop {
        // Cloneable because the bodies are in-memory JSON.
        let spare = if tries < policy.max_retries { req.try_clone() } else { None };
        match attempt(req, abort) {
            Ok(v) => return Ok(v),
            Err(f) => {
                // Redacted before it is logged or returned: a provider may echo the key in a
                // 400/500 body, and the log must never hold it (§10.7).
                let error = redact_error(f.error, secret);
                let Some(next) = spare.filter(|_| f.retryable) else { return Err(error) };
                let wait = policy.delay(tries, f.retry_after);
                // Only the error *kind* is logged: the message may carry up to 200 chars of the provider's
                // body, and a provider can echo the prompt (transcript text) back.
                tracing::warn!("provider de IA: {} — nova tentativa em {} ms ({}/{})", error.action().title, wait.as_millis(), tries + 1, policy.max_retries);
                sleep_abortable(wait, abort)?;
                tries += 1;
                req = next;
            }
        }
    }
}

/// Replaces every occurrence of `secret` in the error's text.
pub(crate) fn redact_error(e: VadError, secret: Option<&str>) -> VadError {
    let Some(secret) = secret.filter(|s| s.len() >= 4) else { return e };
    let r = |m: String| m.replace(secret, "***");
    match e {
        VadError::Llm(m) => VadError::Llm(r(m)),
        VadError::LlmAuthFailed(m) => VadError::LlmAuthFailed(r(m)),
        VadError::LlmTimeout(m) => VadError::LlmTimeout(r(m)),
        VadError::LlmRateLimited(m) => VadError::LlmRateLimited(r(m)),
        VadError::NoNetwork(m) => VadError::NoNetwork(r(m)),
        other => other,
    }
}

/// Non-empty trimmed text or an error: an empty answer is never a valid summary.
pub(crate) fn non_empty(text: Option<&str>, what: &str) -> Result<String, VadError> {
    let text = text.ok_or_else(|| VadError::Llm(format!("resposta sem {what}")))?.trim().to_string();
    if text.is_empty() {
        return Err(VadError::Llm("o provider devolveu texto vazio".into()));
    }
    Ok(text)
}
