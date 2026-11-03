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

fn run(req: RequestBuilder) -> Result<Value, VadError> {
    let resp = req.send().map_err(|e| {
        if e.is_timeout() {
            VadError::LlmTimeout("o provider não respondeu a tempo".into())
        } else if e.is_connect() {
            VadError::Llm(format!("sem ligação ao provider (endereço errado ou offline): {}", e.without_url()))
        } else {
            VadError::Llm(format!("falha de ligação ao provider: {}", e.without_url()))
        }
    })?;
    let status = resp.status();
    if !status.is_success() {
        // Bodies are not echoed on auth errors: some providers repeat part of the key.
        return Err(match status.as_u16() {
            401 | 403 => VadError::LlmAuthFailed(format!("HTTP {status}")),
            429 => VadError::LlmRateLimited("HTTP 429".into()),
            404 => VadError::Llm("HTTP 404: endpoint ou modelo não encontrado — verifica o endereço e o nome do modelo".into()),
            _ => {
                let body = resp.text().unwrap_or_default();
                VadError::Llm(format!("HTTP {status}: {}", body.chars().take(200).collect::<String>()))
            }
        });
    }
    resp.json().map_err(|e| VadError::Llm(format!("resposta do provider não é JSON válido: {e}")))
}

/// Sends `req` and returns the JSON body, honouring `abort` while waiting.
pub(crate) fn execute(req: RequestBuilder, abort: Option<&AtomicBool>) -> Result<Value, VadError> {
    let cancelled = || abort.is_some_and(|a| a.load(Ordering::Relaxed));
    if cancelled() {
        return Err(VadError::LlmCancelled);
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
                    return Err(VadError::LlmCancelled);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err(VadError::Llm("o pedido ao provider terminou inesperadamente".into()));
            }
        }
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
