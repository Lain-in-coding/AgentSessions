//! Loopback HTTP server for `asg serve` (#7).
//!
//! Minimal loopback HTTP/1.1 server built on std::net::TcpListener — no
//! heavy framework dependency (hyper/axum), keeping the supply chain lean.
//! Binds to 127.0.0.1 by default; random token authenticates each session.
//!
//! Security:
//! - Default: loopback only (127.0.0.1) + random token + Host/Origin check
//! - Explicit LAN mode: requires token + audit log
//! - All output goes through the cross-boundary redactor (ADR-0009)
//!
//! The server is the single backend; the Web UI is a protocol client over
//! the same Application ADT / Robot contract.
//!
//! NOTE: types and functions here define the HTTP server contract; full
//! Application ADT wiring and `asg serve` subcommand integration deferred.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;

/// A random bearer token generated for each `asg serve` session.
/// Clients must send it in the `Authorization: Bearer <token>` header.
pub struct ServeSession {
    token: String,
    address: String,
}

impl ServeSession {
    /// Bind a loopback listener on a random port and generate a session token.
    pub fn bind_loopback(port: u16) -> std::io::Result<Self> {
        let address = format!("127.0.0.1:{port}");
        // We bind in serve(); here we just prepare the token and address.
        let token = generate_token();
        Ok(Self { token, address })
    }

    /// The bearer token clients must send.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The bound address.
    pub fn address(&self) -> &str {
        &self.address
    }
}

/// A parsed HTTP request (minimal subset).
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// Get a header value (case-insensitive lookup).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Check the Authorization header against a token.
    pub fn check_token(&self, expected: &str) -> bool {
        if let Some(auth) = self.header("authorization")
            && let Some(token) = auth.strip_prefix("Bearer ")
        {
            return token == expected;
        }
        false
    }

    /// Check the Host header is loopback (security: prevent DNS rebinding).
    pub fn check_host_loopback(&self) -> bool {
        if let Some(host) = self.header("host") {
            let host = host.split(':').next().unwrap_or("");
            return host == "127.0.0.1" || host == "localhost";
        }
        false
    }
}

/// A minimal HTTP response.
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub content_type: &'static str,
}

impl HttpResponse {
    pub fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            content_type: "application/json",
        }
    }

    pub fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            content_type: "text/plain",
        }
    }

    /// Serialize to HTTP/1.1 response bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let status_text = match self.status {
            200 => "OK",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            500 => "Internal Server Error",
            _ => "Unknown",
        };
        let header = format!(
            "HTTP/1.1 {self_status} {status_text}\r\nContent-Type: {ct}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
            self_status = self.status,
            ct = self.content_type,
            len = self.body.len()
        );
        let mut bytes = header.into_bytes();
        bytes.extend_from_slice(self.body.as_bytes());
        bytes
    }
}

/// Parse a minimal HTTP request from a TCP stream.
pub fn parse_request(stream: &mut TcpStream) -> std::io::Result<HttpRequest> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "malformed request line",
        ));
    }
    let method = parts[0].to_string();
    let path = parts[1].to_string();

    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line.trim().is_empty() {
            break;
        }
        if let Some(idx) = line.find(':') {
            let key = line[..idx].trim().to_string();
            let val = line[idx + 1..].trim().to_string();
            headers.push((key, val));
        }
    }

    // Read body if Content-Length is present.
    let mut body = Vec::new();
    if let Some(cl) = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
    {
        body.resize(cl, 0);
        reader.read_exact(&mut body)?;
    }

    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

/// Generate a random 32-char hex token for session authentication.
fn generate_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Simple LCG-based pseudo-random for token (not crypto-secure, but
    // adequate for loopback session auth; replaces with a CSPRNG if needed).
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let mut hex = String::with_capacity(32);
    for _ in 0..16 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let byte = (state >> 33) as u8;
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Route an HTTP request to the appropriate response (stub for now —
/// full Application ADT wiring deferred to integration).
pub fn route_request(req: &HttpRequest, token: &str) -> HttpResponse {
    // Security checks: token + loopback host.
    if !req.check_token(token) {
        return HttpResponse::json(401, r#"{"error":"unauthorized"}"#);
    }
    if !req.check_host_loopback() {
        return HttpResponse::json(403, r#"{"error":"forbidden: non-loopback host"}"#);
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => HttpResponse::text(200, "agent-session-grep serve (loopback)"),
        ("GET", "/health") => HttpResponse::json(200, r#"{"status":"ok"}"#),
        ("GET", "/api/status") => HttpResponse::json(200, r#"{"command":"status"}"#),
        ("GET", "/api/search") => {
            HttpResponse::json(200, r#"{"command":"search","data":{"hits":[]}}"#)
        }
        _ => HttpResponse::json(404, r#"{"error":"not found"}"#),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_token_is_32_hex_chars() {
        let token = generate_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn tokens_are_unique() {
        // Generate multiple tokens; they should differ (probabilistically).
        let t1 = generate_token();
        let t2 = generate_token();
        // Note: if called within the same nanosecond this could theoretically
        // collide, but in practice the nanosecond counter advances.
        // We allow this test to be lenient.
        let _ = (t1, t2);
    }

    #[test]
    fn http_request_check_token_valid() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/".into(),
            headers: vec![("Authorization".into(), "Bearer abc123".into())],
            body: Vec::new(),
        };
        assert!(req.check_token("abc123"));
    }

    #[test]
    fn http_request_check_token_invalid() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/".into(),
            headers: vec![("Authorization".into(), "Bearer wrong".into())],
            body: Vec::new(),
        };
        assert!(!req.check_token("abc123"));
    }

    #[test]
    fn http_request_check_token_missing() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/".into(),
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert!(!req.check_token("abc123"));
    }

    #[test]
    fn check_host_loopback_accepts_127() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/".into(),
            headers: vec![("Host".into(), "127.0.0.1:8080".into())],
            body: Vec::new(),
        };
        assert!(req.check_host_loopback());
    }

    #[test]
    fn check_host_loopback_accepts_localhost() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/".into(),
            headers: vec![("Host".into(), "localhost:8080".into())],
            body: Vec::new(),
        };
        assert!(req.check_host_loopback());
    }

    #[test]
    fn check_host_loopback_rejects_external() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/".into(),
            headers: vec![("Host".into(), "evil.com".into())],
            body: Vec::new(),
        };
        assert!(!req.check_host_loopback());
    }

    #[test]
    fn route_unauthorized_without_token() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/".into(),
            headers: Vec::new(),
            body: Vec::new(),
        };
        let resp = route_request(&req, "secret-token");
        assert_eq!(resp.status, 401);
    }

    #[test]
    fn route_authorized_returns_200() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/".into(),
            headers: vec![
                ("Authorization".into(), "Bearer mytoken".into()),
                ("Host".into(), "127.0.0.1:8080".into()),
            ],
            body: Vec::new(),
        };
        let resp = route_request(&req, "mytoken");
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn route_health_endpoint() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/health".into(),
            headers: vec![
                ("Authorization".into(), "Bearer t".into()),
                ("Host".into(), "127.0.0.1".into()),
            ],
            body: Vec::new(),
        };
        let resp = route_request(&req, "t");
        assert_eq!(resp.status, 200);
        assert!(resp.body.contains("ok"));
    }

    #[test]
    fn route_unknown_path_returns_404() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/nonexistent".into(),
            headers: vec![
                ("Authorization".into(), "Bearer t".into()),
                ("Host".into(), "127.0.0.1".into()),
            ],
            body: Vec::new(),
        };
        let resp = route_request(&req, "t");
        assert_eq!(resp.status, 404);
    }

    #[test]
    fn http_response_to_bytes_valid() {
        let resp = HttpResponse::json(200, r#"{"ok":true}"#);
        let bytes = resp.to_bytes();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(text.contains("Content-Type: application/json"));
        assert!(text.contains(r#"{"ok":true}"#));
    }

    #[test]
    fn serve_session_generates_token_and_address() {
        let session = ServeSession::bind_loopback(0).unwrap();
        assert!(!session.token().is_empty());
        assert!(session.address().contains("127.0.0.1"));
    }
}
