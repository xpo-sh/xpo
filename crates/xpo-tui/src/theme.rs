use ratatui::style::{Color, Modifier, Style};

pub struct Theme;

impl Theme {
    pub const BORDER: Color = Color::Rgb(72, 79, 88);
    pub const TEXT: Color = Color::Rgb(230, 237, 243);
    pub const TEXT_DIM: Color = Color::Rgb(139, 148, 158);
    pub const ACCENT: Color = Color::Rgb(88, 166, 255);
    pub const SUCCESS: Color = Color::Rgb(126, 231, 135);
    pub const ERROR: Color = Color::Rgb(248, 81, 73);
    pub const REDIRECT: Color = Color::Rgb(88, 166, 255);
    pub const METHOD_GET: Color = Color::Rgb(210, 168, 255);
    pub const METHOD_POST: Color = Color::Rgb(121, 192, 255);
    pub const METHOD_PUT: Color = Color::Rgb(224, 175, 104);
    pub const METHOD_DELETE: Color = Color::Rgb(248, 81, 73);
    pub const SPARKLINE: Color = Color::Rgb(88, 166, 255);
    pub const QR_MODULE: Color = Color::Rgb(230, 237, 243);

    pub fn border() -> Style {
        Style::default().fg(Self::BORDER)
    }

    pub fn text() -> Style {
        Style::default().fg(Self::TEXT)
    }

    pub fn text_dim() -> Style {
        Style::default().fg(Self::TEXT_DIM)
    }

    pub fn accent() -> Style {
        Style::default().fg(Self::ACCENT)
    }

    pub fn accent_bold() -> Style {
        Style::default()
            .fg(Self::ACCENT)
            .add_modifier(Modifier::BOLD)
    }

    pub fn success() -> Style {
        Style::default().fg(Self::SUCCESS)
    }

    pub fn error() -> Style {
        Style::default().fg(Self::ERROR)
    }

    pub fn method_style(method: &str) -> Style {
        let color = match method {
            "GET" => Self::METHOD_GET,
            "POST" => Self::METHOD_POST,
            "PUT" => Self::METHOD_PUT,
            "DELETE" => Self::METHOD_DELETE,
            "PATCH" => Self::METHOD_PUT,
            "HEAD" => Self::METHOD_GET,
            "OPTIONS" => Self::TEXT_DIM,
            _ => Self::TEXT,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }

    pub fn status_style(status: u16) -> Style {
        match status {
            500..=599 => Style::default()
                .fg(Self::ERROR)
                .add_modifier(Modifier::BOLD),
            400..=499 => Style::default().fg(Self::ERROR),
            300..=399 => Style::default().fg(Self::REDIRECT),
            200..=299 => Style::default().fg(Self::SUCCESS),
            _ => Style::default().fg(Self::TEXT_DIM),
        }
    }
}
