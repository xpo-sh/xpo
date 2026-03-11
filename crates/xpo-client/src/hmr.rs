use clap::ValueEnum;
use dashmap::DashSet;
use regex::{Captures, Regex};
use std::sync::{Arc, OnceLock};

const MAX_REWRITE_BODY_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HmrMode {
    Off,
    Auto,
}

#[derive(Debug, Clone)]
pub struct HmrContext {
    pub public_host: String,
    pub public_port: u16,
    pub public_scheme: String,
    rewritten_ws_paths: Arc<DashSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HmrRewriteOutcome {
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HmrContext {
    pub fn new(public_host: String, public_port: u16, public_scheme: String) -> Self {
        Self {
            public_host,
            public_port,
            public_scheme,
            rewritten_ws_paths: Arc::new(DashSet::new()),
        }
    }

    pub fn from_tunnel_url(url: &str, mode: HmrMode) -> Option<Self> {
        if mode == HmrMode::Off {
            return None;
        }

        let url = reqwest::Url::parse(url).ok()?;
        let public_host = url.host_str()?.to_string();
        let public_port = url.port_or_known_default().unwrap_or(443);

        Some(Self::new(
            public_host,
            public_port,
            url.scheme().to_string(),
        ))
    }
}

pub fn should_strip_accept_encoding(
    hmr_context: Option<&HmrContext>,
    method: &str,
    path: &str,
) -> bool {
    hmr_context.is_some() && method.eq_ignore_ascii_case("GET") && is_text_asset_path(path)
}

pub fn maybe_rewrite_response(
    hmr_context: Option<&HmrContext>,
    request_path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Option<HmrRewriteOutcome> {
    let hmr_context = hmr_context?;

    if body.len() > MAX_REWRITE_BODY_SIZE || !is_text_response(headers) {
        return None;
    }

    let body_text = String::from_utf8(body.to_vec()).ok()?;
    if !looks_like_webpack_wds_bundle(request_path, &body_text) {
        return None;
    }

    let (rewritten, rewritten_paths) = rewrite_webpack_socket_query(&body_text, hmr_context);
    if rewritten == body_text {
        return None;
    }

    for path in rewritten_paths {
        hmr_context.rewritten_ws_paths.insert(path);
    }

    let mut rewritten_headers = headers.to_vec();
    strip_invalidated_headers(&mut rewritten_headers);

    Some(HmrRewriteOutcome {
        headers: rewritten_headers,
        body: rewritten.into_bytes(),
    })
}

pub fn should_rewrite_ws_origin(hmr_context: Option<&HmrContext>, path: &str) -> bool {
    let hmr_context = match hmr_context {
        Some(ctx) => ctx,
        None => return false,
    };

    hmr_context.rewritten_ws_paths.contains(path)
}

fn is_text_asset_path(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    path.ends_with(".js") || path.ends_with(".mjs") || path.contains("hot-update")
}

fn is_text_response(headers: &[(String, String)]) -> bool {
    let content_type = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.to_ascii_lowercase())
        .unwrap_or_default();

    let content_encoding = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
        .map(|(_, value)| value.to_ascii_lowercase());

    if let Some(encoding) = content_encoding {
        if !encoding.is_empty() && encoding != "identity" {
            return false;
        }
    }

    content_type.contains("text/html")
        || content_type.contains("text/javascript")
        || content_type.contains("application/javascript")
        || content_type.contains("application/x-javascript")
}

fn looks_like_webpack_wds_bundle(request_path: &str, body: &str) -> bool {
    is_text_asset_path(request_path)
        && body.contains("webpack-dev-server/client/index.js?")
        && body.contains("createSocketURL")
        && (body.contains("pathname=%2Fws")
            || body.contains("pathname=/ws")
            || body.contains("sockjs-node"))
}

fn rewrite_webpack_socket_query(body: &str, hmr_context: &HmrContext) -> (String, Vec<String>) {
    static SOCKET_QUERY_RE: OnceLock<Regex> = OnceLock::new();

    let re = SOCKET_QUERY_RE.get_or_init(|| {
        Regex::new(
            r#"protocol=(?:ws|wss|auto)(?:%3A|:)?&(?:(username=[^&"]*&password=[^&"]*&)?hostname=[^&"]+&port=[^&"]+&pathname=([^&"]+))"#,
        )
        .expect("valid webpack socket query regex")
    });

    let mut rewritten_paths = Vec::new();
    let rewritten = re.replace_all(body, |caps: &Captures<'_>| {
        let auth = caps.get(1).map_or("", |m| m.as_str());
        let pathname = caps.get(2).map_or("%2Fws", |m| m.as_str());
        rewritten_paths.push(normalize_ws_path(pathname));
        let protocol = if hmr_context.public_scheme.eq_ignore_ascii_case("https") {
            "wss%3A"
        } else {
            "ws%3A"
        };
        format!(
            "protocol={protocol}&{auth}hostname={}&port={}&pathname={pathname}",
            hmr_context.public_host, hmr_context.public_port
        )
    });

    (rewritten.into_owned(), rewritten_paths)
}

fn strip_invalidated_headers(headers: &mut Vec<(String, String)>) {
    headers.retain(|(name, _)| {
        !name.eq_ignore_ascii_case("etag")
            && !name.eq_ignore_ascii_case("content-md5")
            && !name.eq_ignore_ascii_case("last-modified")
    });
}

fn normalize_ws_path(path: &str) -> String {
    path.replace("%2F", "/").replace("%2f", "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn js_headers() -> Vec<(String, String)> {
        vec![(
            "Content-Type".to_string(),
            "application/javascript".to_string(),
        )]
    }

    #[test]
    fn should_strip_accept_encoding_for_js_assets() {
        let ctx = HmrContext::new("demo.xpo.sh".into(), 443, "https".into());
        assert!(should_strip_accept_encoding(Some(&ctx), "GET", "/app.js"));
        assert!(!should_strip_accept_encoding(Some(&ctx), "POST", "/app.js"));
        assert!(!should_strip_accept_encoding(Some(&ctx), "GET", "/"));
        assert!(!should_strip_accept_encoding(
            Some(&ctx),
            "GET",
            "/styles.css"
        ));
        assert!(!should_strip_accept_encoding(
            Some(&ctx),
            "GET",
            "/api/data"
        ));
        assert!(!should_strip_accept_encoding(None, "GET", "/app.js"));
    }

    #[test]
    fn rewrites_webpack_bundle_with_private_ip() {
        let ctx = HmrContext::new("demo.xpo.sh".into(), 443, "https".into());
        let body = r#"__webpack_require__("./node_modules/webpack-dev-server/client/index.js?protocol=ws%3A&username=&password=&hostname=172.20.10.6&port=8080&pathname=%2Fws&logging=none");function x(){var __resourceQuery="?protocol=ws%3A&username=&password=&hostname=172.20.10.6&port=8080&pathname=%2Fws"}webpack-dev-server/client/index.js?createSocketURL"#;
        let outcome = maybe_rewrite_response(Some(&ctx), "/app.js", &js_headers(), body.as_bytes())
            .expect("must rewrite");
        let text = String::from_utf8(outcome.body).unwrap();
        assert!(text.contains("protocol=wss%3A"));
        assert!(text.contains("hostname=demo.xpo.sh&port=443&pathname=%2Fws"));
        assert!(!text.contains("hostname=172.20.10.6"));
        assert!(should_rewrite_ws_origin(Some(&ctx), "/ws"));
    }

    #[test]
    fn rewrites_webpack_bundle_with_zero_host() {
        let ctx = HmrContext::new("demo.xpo.sh".into(), 443, "https".into());
        let body = r#"var __resourceQuery="?protocol=ws%3A&hostname=0.0.0.0&port=8080&pathname=%2Fws&logging=none";webpack-dev-server/client/index.js?foo;createSocketURL"#;
        let outcome = maybe_rewrite_response(Some(&ctx), "/app.js", &js_headers(), body.as_bytes())
            .expect("must rewrite");
        let text = String::from_utf8(outcome.body).unwrap();
        assert!(text.contains("hostname=demo.xpo.sh&port=443&pathname=%2Fws"));
        assert!(!text.contains("hostname=0.0.0.0"));
    }

    #[test]
    fn does_not_rewrite_unrelated_js() {
        let ctx = HmrContext::new("demo.xpo.sh".into(), 443, "https".into());
        let body = r#"console.log("ws://172.20.10.6:8080/ws");"#;
        assert!(
            maybe_rewrite_response(Some(&ctx), "/app.js", &js_headers(), body.as_bytes()).is_none()
        );
    }

    #[test]
    fn strips_invalidated_headers_when_rewriting() {
        let ctx = HmrContext::new("demo.xpo.sh".into(), 443, "https".into());
        let headers = vec![
            (
                "Content-Type".to_string(),
                "application/javascript".to_string(),
            ),
            ("ETag".to_string(), "abc".to_string()),
            ("Last-Modified".to_string(), "yesterday".to_string()),
        ];
        let body = r#"var __resourceQuery="?protocol=ws%3A&hostname=0.0.0.0&port=8080&pathname=%2Fws";webpack-dev-server/client/index.js?createSocketURL"#;
        let outcome = maybe_rewrite_response(Some(&ctx), "/app.js", &headers, body.as_bytes())
            .expect("must rewrite");
        assert!(!outcome
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("etag")));
        assert!(!outcome
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("last-modified")));
    }

    #[test]
    fn uses_public_scheme_for_rewrite() {
        let ctx = HmrContext::new("localhost".into(), 8080, "http".into());
        let body = r#"var __resourceQuery="?protocol=ws%3A&hostname=0.0.0.0&port=8080&pathname=%2Fws";webpack-dev-server/client/index.js?createSocketURL"#;
        let outcome = maybe_rewrite_response(Some(&ctx), "/app.js", &js_headers(), body.as_bytes())
            .expect("must rewrite");
        let text = String::from_utf8(outcome.body).unwrap();
        assert!(text.contains("protocol=ws%3A&hostname=localhost&port=8080"));
    }
}
