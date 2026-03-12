pub fn parse_http_headers(raw: &[u8]) -> Vec<(String, String)> {
    let s = String::from_utf8_lossy(raw);
    let mut headers = Vec::new();
    for line in s.split("\r\n").skip(1) {
        if line.is_empty() {
            break;
        }
        if let Some((key, val)) = line.split_once(':') {
            headers.push((key.trim().to_string(), val.trim().to_string()));
        }
    }
    headers
}

pub fn extract_body_preview(body: &[u8], max_bytes: usize) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    let limit = max_bytes.min(body.len());
    let safe_end = safe_utf8_boundary(body, limit);
    let preview = String::from_utf8_lossy(&body[..safe_end]).into_owned();
    Some(preview)
}

fn safe_utf8_boundary(bytes: &[u8], max: usize) -> usize {
    if max >= bytes.len() {
        return bytes.len();
    }
    let mut end = max;
    while end > 0 && (bytes[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    end
}

pub fn content_type_to_extension(headers: &[(String, String)]) -> &'static str {
    let ct = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str())
        .unwrap_or("");

    if ct.contains("json") {
        ".json"
    } else if ct.contains("html") {
        ".html"
    } else if ct.contains("xml") {
        ".xml"
    } else if ct.contains("javascript") {
        ".js"
    } else if ct.contains("css") {
        ".css"
    } else {
        ".txt"
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;

    #[test]
    fn parse_http_headers_basic() {
        let raw = b"GET / HTTP/1.1\r\nHost: example.com\r\nAccept: */*\r\n\r\n";
        let headers = parse_http_headers(raw);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0], ("Host".to_string(), "example.com".to_string()));
        assert_eq!(headers[1], ("Accept".to_string(), "*/*".to_string()));
    }

    #[test]
    fn parse_http_headers_empty() {
        let raw = b"GET / HTTP/1.1\r\n\r\n";
        let headers = parse_http_headers(raw);
        assert!(headers.is_empty());
    }

    #[test]
    fn extract_body_preview_truncates() {
        let body = "a".repeat(8000);
        let preview = extract_body_preview(body.as_bytes(), 4096).unwrap();
        assert_eq!(preview.len(), 4096);
    }

    #[test]
    fn extract_body_preview_empty() {
        assert!(extract_body_preview(b"", 4096).is_none());
    }

    #[test]
    fn extract_body_preview_utf8_safe() {
        let body = "Hello \u{1F600} world";
        let bytes = body.as_bytes();
        let preview = extract_body_preview(bytes, 8).unwrap();
        assert_eq!(preview, "Hello ");
    }

    #[test]
    fn content_type_to_extension_json() {
        let headers = vec![(
            "Content-Type".to_string(),
            "application/json; charset=utf-8".to_string(),
        )];
        assert_eq!(content_type_to_extension(&headers), ".json");
    }

    #[test]
    fn content_type_to_extension_html() {
        let headers = vec![("Content-Type".to_string(), "text/html".to_string())];
        assert_eq!(content_type_to_extension(&headers), ".html");
    }

    #[test]
    fn content_type_to_extension_default() {
        let headers = vec![(
            "Content-Type".to_string(),
            "application/octet-stream".to_string(),
        )];
        assert_eq!(content_type_to_extension(&headers), ".txt");
    }

    #[test]
    fn content_type_to_extension_missing() {
        let headers: Vec<(String, String)> = vec![];
        assert_eq!(content_type_to_extension(&headers), ".txt");
    }
}
