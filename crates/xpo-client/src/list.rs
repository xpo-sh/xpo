use serde::Serialize;

#[derive(Serialize)]
struct ListEntry {
    #[serde(rename = "type")]
    kind: String,
    domain: String,
    target: String,
    status: String,
}

pub async fn run(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut entries = Vec::new();

    let config = xpo_core::config::Config::load().unwrap_or_default();
    if config.is_authenticated() && !config.is_expired() {
        if let Ok(token) = crate::auth::get_token().await {
            let server = std::env::var("XPO_API_SERVER")
                .unwrap_or_else(|_| "https://api.xpo.sh".to_string());
            let client = reqwest::Client::new();
            if let Ok(resp) = client
                .get(format!("{}/api/tunnels", server))
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
            {
                if let Ok(tunnels) = resp.json::<Vec<serde_json::Value>>().await {
                    for t in tunnels {
                        let subdomain = t["subdomain"].as_str().unwrap_or("?");
                        let url = t["url"].as_str().unwrap_or("?");
                        let port = t["port"].as_u64().unwrap_or(0);
                        entries.push(ListEntry {
                            kind: "share".to_string(),
                            domain: url.to_string(),
                            target: format!("localhost:{}", port),
                            status: "active".to_string(),
                        });
                        let _ = subdomain;
                    }
                }
            }
        }
    }

    let hosts_content = std::fs::read_to_string("/etc/hosts").unwrap_or_default();
    let test_domains = parse_test_domains(&hosts_content);
    for domain in test_domains {
        let port = 443u16;
        let active = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .is_ok();
        entries.push(ListEntry {
            kind: "dev".to_string(),
            domain: domain.clone(),
            target: format!("localhost:{}", port),
            status: if active {
                "active".to_string()
            } else {
                "inactive".to_string()
            },
        });
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else if entries.is_empty() {
        xpo_tui::widgets::list_table::render_empty();
    } else {
        let rows: Vec<xpo_tui::widgets::list_table::ListRow> = entries
            .iter()
            .map(|e| xpo_tui::widgets::list_table::ListRow {
                kind: e.kind.clone(),
                domain: e.domain.clone(),
                target: e.target.clone(),
                status: e.status.clone(),
            })
            .collect();
        xpo_tui::widgets::list_table::render_list_table(&rows)?;
    }

    Ok(())
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
}
