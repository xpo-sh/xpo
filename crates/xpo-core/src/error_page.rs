fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

pub struct ErrorPage {
    pub status_code: u16,
    pub title: String,
    pub message: String,
    pub hint: String,
    pub brand: String,
    pub escape_message: bool,
}

impl ErrorPage {
    pub fn new(status_code: u16, message: &str) -> Self {
        Self {
            status_code,
            title: String::new(),
            message: message.to_string(),
            hint: String::new(),
            brand: "<span>xpo</span>.sh".to_string(),
            escape_message: true,
        }
    }

    pub fn title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }

    pub fn hint(mut self, hint: &str) -> Self {
        self.hint = hint.to_string();
        self
    }

    pub fn brand(mut self, brand: &str) -> Self {
        self.brand = brand.to_string();
        self
    }

    pub fn raw_html_message(mut self) -> Self {
        self.escape_message = false;
        self
    }

    pub fn render_html(&self) -> String {
        let code = self.status_code;

        let safe_message = if self.escape_message {
            html_escape(&self.message)
        } else {
            self.message.clone()
        };

        let hint_html = if self.hint.is_empty() {
            String::new()
        } else {
            format!(
                "<p class=\"hint\">{}</p>",
                if self.escape_message {
                    html_escape(&self.hint)
                } else {
                    self.hint.clone()
                }
            )
        };

        let page_title = if self.title.is_empty() {
            format!("{code} - xpo.sh")
        } else {
            format!("{code} {}", html_escape(&self.title))
        };

        format!(
            "<!DOCTYPE html>\
            <html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
            <title>{page_title}</title>\
            <style>\
            *{{margin:0;padding:0;box-sizing:border-box}}\
            body{{font-family:'JetBrains Mono','Fira Code','SF Mono',Menlo,Consolas,monospace;\
            display:flex;align-items:center;justify-content:center;height:100vh;\
            background:#0a0a0f;color:#e2e2e8}}\
            .c{{text-align:center}}\
            .code{{font-size:96px;font-weight:800;line-height:1;color:#1e1e2e}}\
            .msg{{margin:16px 0 0;font-size:15px}}\
            .msg b{{color:#22d3ee}}\
            .hint{{color:#555570;font-size:13px;margin:8px 0 0}}\
            a{{color:#22d3ee;text-decoration:none}}\
            a:hover{{text-decoration:underline}}\
            .brand{{position:fixed;bottom:24px;color:#555570;font-size:12px}}\
            .brand span{{color:#22d3ee;font-weight:600}}\
            @media(prefers-color-scheme:light){{\
            body{{background:#f5f6f8;color:#111827}}\
            .code{{color:#e2e4e9}}\
            .msg b{{color:#0891b2}}\
            .hint{{color:#6b7280}}\
            a{{color:#0891b2}}\
            .brand{{color:#6b7280}}\
            .brand span{{color:#0891b2}}\
            }}\
            </style></head>\
            <body><div class=\"c\">\
            <p class=\"code\">{code}</p>\
            <p class=\"msg\">{safe_message}</p>\
            {hint_html}\
            </div>\
            <div class=\"brand\">{}</div>\
            </body></html>",
            self.brand
        )
    }

    pub fn status_text(&self) -> &str {
        match self.status_code {
            400 => "Bad Request",
            404 => "Not Found",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            504 => "Gateway Timeout",
            _ => {
                if !self.title.is_empty() {
                    return &self.title;
                }
                "Error"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escape_special_chars() {
        assert_eq!(
            html_escape("<script>alert(1)</script>"),
            "&lt;script&gt;alert(1)&lt;/script&gt;"
        );
        assert_eq!(html_escape("a&b\"c'd"), "a&amp;b&quot;c&#x27;d");
    }

    #[test]
    fn html_escape_noop_on_clean_text() {
        let clean = "normal-sub.xpo.sh is not connected";
        assert_eq!(html_escape(clean), clean);
    }

    #[test]
    fn render_basic_error_page() {
        let html = ErrorPage::new(502, "Bad Gateway").render_html();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<p class=\"code\">502</p>"));
        assert!(html.contains("Bad Gateway"));
        assert!(html.contains("<span>xpo</span>.sh"));
    }

    #[test]
    fn render_with_hint() {
        let html = ErrorPage::new(404, "Tunnel not found")
            .hint("The tunnel may have been closed")
            .render_html();
        assert!(html.contains("Tunnel not found"));
        assert!(html.contains("The tunnel may have been closed"));
    }

    #[test]
    fn render_without_hint() {
        let html = ErrorPage::new(502, "test").render_html();
        assert!(!html.contains("class=\"hint\""));
    }

    #[test]
    fn render_custom_brand() {
        let html = ErrorPage::new(502, "test")
            .brand("<span>xpo</span> dev")
            .render_html();
        assert!(html.contains("<span>xpo</span> dev"));
    }

    #[test]
    fn render_escapes_xss() {
        let html = ErrorPage::new(502, "<script>alert(1)</script>").render_html();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn render_raw_html_message() {
        let html = ErrorPage::new(502, "Cannot reach <b>localhost:3000</b>")
            .raw_html_message()
            .render_html();
        assert!(html.contains("<b>localhost:3000</b>"));
    }

    #[test]
    fn render_with_title() {
        let html = ErrorPage::new(502, "msg")
            .title("Bad Gateway")
            .render_html();
        assert!(html.contains("<title>502 Bad Gateway</title>"));
    }

    #[test]
    fn render_without_title() {
        let html = ErrorPage::new(502, "msg").render_html();
        assert!(html.contains("<title>502 - xpo.sh</title>"));
    }

    #[test]
    fn has_dark_and_light_theme() {
        let html = ErrorPage::new(502, "test").render_html();
        assert!(html.contains("background:#0a0a0f"));
        assert!(html.contains("prefers-color-scheme:light"));
        assert!(html.contains("background:#f5f6f8"));
    }

    #[test]
    fn status_text_known_codes() {
        assert_eq!(ErrorPage::new(404, "").status_text(), "Not Found");
        assert_eq!(ErrorPage::new(502, "").status_text(), "Bad Gateway");
        assert_eq!(ErrorPage::new(504, "").status_text(), "Gateway Timeout");
    }

    #[test]
    fn status_text_unknown_with_title() {
        assert_eq!(
            ErrorPage::new(599, "").title("Custom").status_text(),
            "Custom"
        );
    }

    #[test]
    fn status_text_unknown_no_title() {
        assert_eq!(ErrorPage::new(599, "").status_text(), "Error");
    }
}
