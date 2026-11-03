//! Tiny HTTP server for provider tests.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// One-shot-per-request HTTP server. Returns `(base_url, captured_requests)`.
pub(crate) fn serve(status: &'static str, body: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    serve_seq(&[(status, body, "")])
}

/// Answers request N with `script[min(N, last)]`: `(status, body, extra_header_line)`.
pub(crate) fn serve_seq(script: &[(&'static str, &'static str, &'static str)]) -> (String, Arc<Mutex<Vec<String>>>) {
    let script: Vec<_> = script.to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = Arc::clone(&seen);
    std::thread::spawn(move || {
        for (n_req, stream) in listener.incoming().enumerate() {
            let Ok(mut s) = stream else { break };
            let (status, body, extra) = script[n_req.min(script.len() - 1)];
            let mut buf = vec![0u8; 16384];
            let mut got = 0;
            // Read headers, then the body announced by Content-Length.
            loop {
                let n = s.read(&mut buf[got..]).unwrap_or(0);
                if n == 0 { break; }
                got += n;
                let text = String::from_utf8_lossy(&buf[..got]).to_string();
                if let Some(h) = text.find("\r\n\r\n") {
                    let len = text[..h].to_lowercase().split("content-length:").nth(1)
                        .and_then(|r| r.lines().next()).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                    if got >= h + 4 + len { break; }
                }
            }
            seen2.lock().unwrap().push(String::from_utf8_lossy(&buf[..got]).to_string());
            let resp = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = s.write_all(resp.as_bytes());
        }
    });
    (format!("http://127.0.0.1:{port}/v1"), seen)
}

