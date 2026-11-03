//! A minimal localhost HTTP server. Each request is handed to the GPUI main
//! thread through a channel and answered from there.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

const MAX_BODY: usize = 1 << 20;

pub struct Request {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
    pub reply: mpsc::Sender<Reply>,
}

pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(status: u16, v: &serde_json::Value) -> Reply {
        Reply { status, content_type: "application/json", body: v.to_string().into_bytes() }
    }
    pub fn png(body: Vec<u8>) -> Reply {
        Reply { status: 200, content_type: "image/png", body }
    }
    pub fn error(status: u16, message: &str) -> Reply {
        Reply::json(status, &serde_json::json!({ "error": message }))
    }
}

/// Bind 127.0.0.1 (`port` 0 picks a free one) and serve on a background
/// thread. Returns the bound port.
pub fn start(port: u16, tx: mpsc::Sender<Request>) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let bound = listener.local_addr()?.port();
    std::thread::Builder::new().name("impeccable-probe".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || serve(stream, tx));
        }
    })?;
    Ok(bound)
}

fn serve(mut stream: TcpStream, tx: mpsc::Sender<Request>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Some((method, path, body, host, origin)) = read_request(&stream) else { return };
    let port = stream.local_addr().map(|a| a.port()).unwrap_or(0);
    if !trusted(host.as_deref(), origin.is_some(), port) {
        let reply = b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(reply);
        return;
    }
    let (reply_tx, reply_rx) = mpsc::channel();
    if tx.send(Request { method, path, body, reply: reply_tx }).is_err() {
        return;
    }
    let reply = reply_rx
        .recv_timeout(Duration::from_secs(180))
        .unwrap_or_else(|_| Reply::error(504, "the app did not answer in time"));
    let reason = match reply.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.status,
        reason,
        reply.content_type,
        reply.body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&reply.body);
}

/// A browser page can reach 127.0.0.1 too. Its requests carry an `Origin`,
/// and a DNS-rebound one a foreign `Host`; the CLI sends neither.
fn trusted(host: Option<&str>, has_origin: bool, port: u16) -> bool {
    let host_ok = host.is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"));
    host_ok && !has_origin
}

type Parsed = (String, String, Vec<u8>, Option<String>, Option<String>);

fn read_request(stream: &TcpStream) -> Option<Parsed> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut length = 0usize;
    let (mut host, mut origin) = (None, None);
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((k, v)) = header.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().ok()?;
            } else if k.eq_ignore_ascii_case("host") {
                host = Some(v.trim().to_string());
            } else if k.eq_ignore_ascii_case("origin") {
                origin = Some(v.trim().to_string());
            }
        }
    }
    if length > MAX_BODY {
        return None;
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some((method, path, body, host, origin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn exchange(raw: &[u8]) -> String {
        let raw = raw.to_vec();
        exchange_to(move |_| String::from_utf8(raw.clone()).unwrap())
    }

    fn exchange_to(raw: impl Fn(u16) -> String) -> String {
        let (tx, rx) = mpsc::channel::<Request>();
        let port = start(0, tx).unwrap();
        let raw = raw(port);
        let raw = raw.as_bytes();
        std::thread::spawn(move || {
            if let Ok(req) = rx.recv_timeout(Duration::from_secs(5)) {
                let body = String::from_utf8_lossy(&req.body).to_string();
                let _ = req.reply.send(Reply::json(201, &serde_json::json!({ "method": req.method, "path": req.path, "body": body })));
            }
        });
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        stream.write_all(raw).unwrap();
        let mut out = String::new();
        let _ = stream.read_to_string(&mut out);
        out
    }

    #[test]
    fn requests_reach_the_app_and_replies_come_back() {
        let out = exchange_to(|port| format!("POST /variants HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 7\r\n\r\n{{\"a\":1}}"));
        assert!(out.starts_with("HTTP/1.1 201 "), "{out}");
        assert!(out.contains("Content-Type: application/json"));
        let body = out.split("\r\n\r\n").nth(1).unwrap();
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(v, serde_json::json!({ "method": "POST", "path": "/variants", "body": "{\"a\":1}" }));
    }

    #[test]
    fn browser_pages_and_foreign_hosts_are_refused() {
        let page = exchange_to(|port| format!("POST /resize HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: https://evil.example\r\nContent-Length: 2\r\n\r\n{{}}"));
        assert!(page.starts_with("HTTP/1.1 403"), "{page}");
        let rebound = exchange(b"GET /capture HTTP/1.1\r\nHost: evil.example:80\r\n\r\n");
        assert!(rebound.starts_with("HTTP/1.1 403"), "{rebound}");
        assert!(trusted(Some("localhost:4100"), false, 4100));
    }

    #[test]
    fn oversized_bodies_are_dropped() {
        let out = exchange_to(|port| format!("POST /variants HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1));
        assert_eq!(out, "");
    }
}
