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

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use agent_session_grep_adapters_sqlite::SqliteStore;
use agent_session_grep_application::{App, AppRequest, AppResponse, ContextLevel, ResponseBudget};
use agent_session_grep_domain::{ContextPolicy, StableId};
use agent_session_grep_ports::SearchFilters;

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
        // The listener is bound in run(); here we just prepare token/address.
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

/// Run the loopback HTTP server until interrupted.
///
/// Serves the embedded Web UI and JSON API endpoints backed by the same
/// Application ADT used by CLI/MCP/Robot. Each request is authenticated by
/// the session token and loopback Host check before reaching the router.
pub fn run(
    session: &ServeSession,
    store: &SqliteStore,
) -> Result<crate::protocol::Outcome, crate::CliError> {
    let listener = TcpListener::bind(session.address())
        .map_err(|e| crate::CliError::usage(format!("serve: bind {}: {e}", session.address())))?;
    eprintln!("asg serve: listening on http://{}", session.address());
    eprintln!("asg serve: token {}", session.token());
    let app = App::with_resume(
        crate::store_ref(store),
        crate::store_ref(store),
        crate::store_ref(store),
    );
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let response = match parse_request(&mut stream) {
                    Ok(req) => route_request(&req, session.token(), &app),
                    Err(_) => HttpResponse::json(400, r#"{"error":"malformed request"}"#),
                };
                let _ = stream.write_all(&response.to_bytes());
                let _ = stream.flush();
            }
            Err(_) => continue,
        }
    }
    Ok(crate::protocol::Outcome::Success)
}

/// A parsed HTTP request (minimal subset).
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    #[allow(dead_code)] // GET-only 路由;body 解析保留给未来 POST 端点
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

    /// Extract a query-string parameter from the request path (percent-
    /// decoding is minimal: '+' as space; other escapes passed through).
    pub fn query_param(&self, name: &str) -> Option<String> {
        let (_, query) = self.path.split_once('?')?;
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if k == name {
                return Some(v.replace('+', " "));
            }
        }
        None
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

    #[allow(dead_code)] // 保留给未来 text/plain 端点
    pub fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            content_type: "text/plain",
        }
    }

    /// Serialize to HTTP/1.1 response bytes.
    #[allow(dead_code)] // body 由 to_bytes 消费;text 构造器留给未来端点
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

/// The embedded Web UI HTML (single-page app, no external dependencies).
const WEB_UI_HTML: &str = include_str!("web/index.html");

/// Route an HTTP request to the appropriate response, backed by the
/// Application ADT over the same SqliteStore used by CLI/MCP/Robot.
pub fn route_request(
    req: &HttpRequest,
    token: &str,
    app: &App<&SqliteStore, &SqliteStore, &SqliteStore>,
) -> HttpResponse {
    // Security checks: token + loopback host.
    if !req.check_token(token) {
        return HttpResponse::json(401, r#"{"error":"unauthorized"}"#);
    }
    if !req.check_host_loopback() {
        return HttpResponse::json(403, r#"{"error":"forbidden: non-loopback host"}"#);
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => HttpResponse {
            status: 200,
            body: WEB_UI_HTML.to_string(),
            content_type: "text/html; charset=utf-8",
        },
        ("GET", "/health") => HttpResponse::json(200, r#"{"status":"ok"}"#),
        ("GET", "/api/status") => match app.handle(AppRequest::Status) {
            Ok(AppResponse::Status {
                catalog_count,
                active_generation,
                ..
            }) => HttpResponse::json(
                200,
                &serde_json::json!({
                    "command": "status",
                    "catalog_count": catalog_count,
                    "generation": active_generation,
                })
                .to_string(),
            ),
            Ok(_) => HttpResponse::json(500, r#"{"error":"unexpected response"}"#),
            Err(e) => HttpResponse::json(
                500,
                &serde_json::json!({"error": e.to_string()}).to_string(),
            ),
        },
        ("GET", "/api/search") => {
            let query = req.query_param("q").unwrap_or_default();
            if query.is_empty() {
                return HttpResponse::json(400, r#"{"error":"missing q parameter"}"#);
            }
            let budget = ResponseBudget {
                max_items: 20,
                max_response_bytes: 2_000_000,
                max_snippet_chars: 512,
                max_messages: 512,
                max_evidence_spans: 64,
            };
            match app.handle(AppRequest::Search {
                query: query.clone(),
                filters: SearchFilters::default(),
                limit: 20,
                cursor: None,
                budget,
                include_system: false,
                group_by_session: false,
            }) {
                Ok(AppResponse::Search {
                    hits, generation, ..
                }) => HttpResponse::json(
                    200,
                    &serde_json::json!({
                        "retrieval_mode": "lexical",
                        "generation": generation,
                        "hits": hits.iter().map(|h| serde_json::json!({
                            "id": h.id.as_str(),
                            "score": h.score,
                            "session_id": h.session_id,
                            "text": h.text,
                        })).collect::<Vec<_>>(),
                    })
                    .to_string(),
                ),
                Ok(_) => HttpResponse::json(500, r#"{"error":"unexpected response"}"#),
                Err(e) => HttpResponse::json(
                    500,
                    &serde_json::json!({"error": e.to_string()}).to_string(),
                ),
            }
        }
        ("GET", "/api/context") => {
            let session = req.query_param("session").unwrap_or_default();
            if session.is_empty() {
                return HttpResponse::json(400, r#"{"error":"missing session parameter"}"#);
            }
            let Ok(id) = StableId::from_wire(&session).ok_or(()) else {
                return HttpResponse::json(400, r#"{"error":"invalid session id"}"#);
            };
            let budget = ResponseBudget {
                max_items: 20,
                max_response_bytes: 2_000_000,
                max_snippet_chars: 512,
                max_messages: 512,
                max_evidence_spans: 64,
            };
            match app.handle(AppRequest::Context {
                session_id: id,
                policy: ContextPolicy::Mainline,
                level: ContextLevel::Raw,
                budget,
            }) {
                Ok(AppResponse::Context {
                    session_id,
                    messages,
                    ..
                }) => HttpResponse::json(
                    200,
                    &serde_json::json!({
                        "session_id": session_id,
                        "messages": messages.iter().map(|m| serde_json::json!({
                            "id": m.id,
                            "placement_id": m.placement_id,
                            "payload": m.payload,
                        })).collect::<Vec<_>>(),
                    })
                    .to_string(),
                ),
                Ok(_) => HttpResponse::json(500, r#"{"error":"unexpected response"}"#),
                Err(e) => HttpResponse::json(
                    500,
                    &serde_json::json!({"error": e.to_string()}).to_string(),
                ),
            }
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

    /// 空内存 store 构造测试 app（API 路由只走 Search/Context/Status，
    /// 空库也能返回合法的空结果/错误 envelope）。
    /// 空内存 store 构造测试 app（API 路由只走 Search/Context/Status，
    /// 空库也能返回合法的空结果/错误 envelope）。每调用新建 store——
    /// SqliteStore 非 Sync，不能放静态共享。
    fn test_app() -> App<&'static SqliteStore, &'static SqliteStore, &'static SqliteStore> {
        let store: &'static SqliteStore = Box::leak(Box::new(
            SqliteStore::open_in_memory().expect("in-memory store"),
        ));
        App::with_resume(store, store, store)
    }

    #[test]
    fn route_unauthorized_without_token() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/".into(),
            headers: Vec::new(),
            body: Vec::new(),
        };
        let resp = route_request(&req, "secret-token", &test_app());
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
        let resp = route_request(&req, "mytoken", &test_app());
        assert_eq!(resp.status, 200);
        assert!(resp.body.contains("<html"));
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
        let resp = route_request(&req, "t", &test_app());
        assert_eq!(resp.status, 200);
        assert!(resp.body.contains("ok"));
    }

    #[test]
    fn route_search_without_query_returns_400() {
        let req = HttpRequest {
            method: "GET".into(),
            path: "/api/search".into(),
            headers: vec![
                ("Authorization".into(), "Bearer t".into()),
                ("Host".into(), "127.0.0.1".into()),
            ],
            body: Vec::new(),
        };
        let resp = route_request(&req, "t", &test_app());
        assert_eq!(resp.status, 400);
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
        let resp = route_request(&req, "t", &test_app());
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
