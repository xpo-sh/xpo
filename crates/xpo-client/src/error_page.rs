pub fn error_response(status_code: u16, message: &str, brand: &str) -> Vec<u8> {
    let status_text = match status_code {
        502 => "Bad Gateway",
        504 => "Gateway Timeout",
        _ => "Error",
    };
    let body = format!(
        "<!DOCTYPE html>\
        <html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
        <title>{status_code} - xpo.sh</title>\
        <style>\
        *{{margin:0;padding:0;box-sizing:border-box}}\
        body{{font-family:'JetBrains Mono','Fira Code','SF Mono',Menlo,Consolas,monospace;\
        display:flex;align-items:center;justify-content:center;height:100vh;\
        background:#0a0a0f;color:#e2e2e8}}\
        .c{{text-align:center}}\
        .code{{font-size:96px;font-weight:800;line-height:1;color:#1e1e2e}}\
        .msg{{margin:16px 0 0;font-size:15px}}\
        .hint{{color:#555570;font-size:13px;margin:8px 0 0}}\
        a{{color:#22d3ee;text-decoration:none}}\
        a:hover{{text-decoration:underline}}\
        .brand{{position:fixed;bottom:24px;color:#555570;font-size:12px}}\
        .brand span{{color:#22d3ee;font-weight:600}}\
        @media(prefers-color-scheme:light){{\
        body{{background:#f5f6f8;color:#111827}}\
        .code{{color:#e2e4e9}}\
        .hint{{color:#6b7280}}\
        a{{color:#0891b2}}\
        .brand{{color:#6b7280}}\
        .brand span{{color:#0891b2}}\
        }}\
        </style></head>\
        <body><div class=\"c\">\
        <p class=\"code\">{status_code}</p>\
        <p class=\"msg\">{message}</p>\
        <p class=\"hint\"><a href=\"https://xpo.sh\">xpo.sh</a></p>\
        </div>\
        <div class=\"brand\"><span>xpo</span> {brand}</div>\
        </body></html>"
    );
    format!(
        "HTTP/1.1 {status_code} {status_text}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .into_bytes()
}
