use xpo_core::error_page::ErrorPage;

pub fn error_response(status_code: u16, message: &str, brand: &str) -> Vec<u8> {
    let page = ErrorPage::new(status_code, message)
        .hint("<a href=\"https://xpo.sh\">xpo.sh</a>")
        .brand(&format!("<span>xpo</span> {brand}"))
        .raw_html_message();
    let body = page.render_html();
    let status_text = page.status_text();
    format!(
        "HTTP/1.1 {status_code} {status_text}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    )
    .into_bytes()
}
