use serde::Serialize;
use xpo_tui::widgets::list_table::ListRow;

#[derive(Serialize)]
struct ListEntry {
    #[serde(rename = "type")]
    kind: String,
    domain: String,
    target: String,
    status: String,
}

pub async fn run(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut rows = Vec::new();
    let mut json_entries = Vec::new();

    let config = xpo_core::config::Config::load().unwrap_or_default();
    if config.is_authenticated() {
        if let Some(token) = config.auth.access_token.as_deref() {
            let server = std::env::var("XPO_API_SERVER")
                .unwrap_or_else(|_| format!("https://{}", config.defaults.server));
            if !json {
                eprint!("\x1b[38;2;139;148;158m  Fetching tunnels...\x1b[0m");
            }
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap_or_default();
            let api_result = client
                .get(format!("{}/api/tunnels", server))
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await;
            if !json {
                eprint!("\r\x1b[2K");
            }
            if let Ok(resp) = api_result {
                if let Ok(tunnels) = resp.json::<Vec<serde_json::Value>>().await {
                    for t in tunnels {
                        let subdomain = t["subdomain"].as_str().unwrap_or("?");
                        let url = t["url"].as_str().unwrap_or("?").to_string();
                        let port = t["port"].as_u64().unwrap_or(0);
                        let has_password = t["has_password"].as_bool().unwrap_or(false);
                        let ttl_secs = t["ttl_secs"].as_u64();
                        let ttl_remaining = t["ttl_remaining_secs"].as_u64();
                        let uptime = t["created_at_secs"].as_u64().unwrap_or(0);

                        let mut details = Vec::new();
                        details.push(("Subdomain".to_string(), subdomain.to_string()));
                        details.push((
                            "Password".to_string(),
                            if has_password { "yes" } else { "no" }.to_string(),
                        ));
                        if let Some(ttl) = ttl_secs {
                            details.push(("TTL".to_string(), format_duration(ttl)));
                        }
                        if let Some(rem) = ttl_remaining {
                            details.push(("Remaining".to_string(), format_duration(rem)));
                        }
                        details.push(("Uptime".to_string(), format_duration(uptime)));

                        let target = format!("localhost:{}", port);
                        rows.push(ListRow {
                            kind: "share".to_string(),
                            domain: url.clone(),
                            target: target.clone(),
                            status: "active".to_string(),
                            details,
                        });
                        json_entries.push(ListEntry {
                            kind: "share".to_string(),
                            domain: url,
                            target,
                            status: "active".to_string(),
                        });
                    }
                }
            }
        }
    }
    drop(config);

    let hosts_content = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
    let test_domains = parse_test_domains(&hosts_content);
    for domain in test_domains {
        let active = tokio::net::TcpStream::connect("127.0.0.1:10443")
            .await
            .is_ok();
        let status = if active { "active" } else { "inactive" };

        let mut details = Vec::new();
        let cert_path = xpo_core::config::Config::dir()
            .join("ca/certs")
            .join(format!("{}.pem", domain));
        if cert_path.exists() {
            details.push(("Cert".to_string(), cert_path.display().to_string()));
        }
        details.push(("Hosts".to_string(), "/etc/hosts".to_string()));
        details.push(("Proxy".to_string(), "https -> localhost:10443".to_string()));

        rows.push(ListRow {
            kind: "dev".to_string(),
            domain: domain.clone(),
            target: format!("https://{}", domain),
            status: status.to_string(),
            details,
        });
        json_entries.push(ListEntry {
            kind: "dev".to_string(),
            domain,
            target: "https (local proxy)".to_string(),
            status: status.to_string(),
        });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&json_entries)?);
        return Ok(());
    }

    xpo_tui::list_app::run(rows)?;
    Ok(())
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn parse_test_domains(content: &str) -> Vec<String> {
    let mut domains = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        if parts[0] != "127.0.0.1" {
            continue;
        }
        for hostname in &parts[1..] {
            if hostname.starts_with('#') {
                break;
            }
            if hostname.ends_with(".test") {
                domains.push(hostname.to_string());
            }
        }
    }
    domains
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_test_domains_basic() {
        let content = "127.0.0.1 myapp.test\n127.0.0.1 other.test\n";
        let result = parse_test_domains(content);
        assert_eq!(result, vec!["myapp.test", "other.test"]);
    }

    #[test]
    fn parse_test_domains_comments() {
        let content = "# this is a comment\n127.0.0.1 myapp.test\n# another comment\n";
        let result = parse_test_domains(content);
        assert_eq!(result, vec!["myapp.test"]);
    }

    #[test]
    fn parse_test_domains_multiple_per_line() {
        let content = "127.0.0.1 one.test two.test three.test\n";
        let result = parse_test_domains(content);
        assert_eq!(result, vec!["one.test", "two.test", "three.test"]);
    }

    #[test]
    fn parse_test_domains_empty() {
        let result = parse_test_domains("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_test_domains_ignores_non_test() {
        let content = "127.0.0.1 localhost\n127.0.0.1 myapp.test\n192.168.1.1 server.test\n";
        let result = parse_test_domains(content);
        assert_eq!(result, vec!["myapp.test"]);
    }

    #[test]
    fn parse_test_domains_inline_comments() {
        let content = "127.0.0.1 myapp.test # added by xpo\n";
        let result = parse_test_domains(content);
        assert_eq!(result, vec!["myapp.test"]);
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(45), "45s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(125), "2m 5s");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(7380), "2h 3m");
    }
}
