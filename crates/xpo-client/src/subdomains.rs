use serde::Deserialize;
use xpo_core::config::Config;
use xpo_tui::subdomains_app::{SubdomainRow, SubdomainsData};

#[derive(Debug, Deserialize)]
struct ApiResponse {
    subdomains: Vec<ApiSubdomain>,
    limit: i32,
    count: usize,
}

#[derive(Debug, Deserialize)]
struct ApiSubdomain {
    subdomain: String,
    created_at: String,
}

fn api_base(config: &Config) -> String {
    std::env::var("XPO_API_SERVER")
        .unwrap_or_else(|_| format!("https://{}", config.defaults.server))
}

fn format_age(created_at: &str) -> String {
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%dT%H:%M:%S%.f") {
            let now = chrono::Utc::now().naive_utc();
            return format_duration(now.signed_duration_since(dt));
        }
        return String::new();
    };
    format_duration(chrono::Utc::now().signed_duration_since(dt))
}

fn format_duration(dur: chrono::TimeDelta) -> String {
    if dur.num_days() > 0 {
        format!("{}d ago", dur.num_days())
    } else if dur.num_hours() > 0 {
        format!("{}h ago", dur.num_hours())
    } else if dur.num_minutes() > 0 {
        format!("{}m ago", dur.num_minutes())
    } else {
        "just now".to_string()
    }
}

fn to_tui_data(resp: ApiResponse) -> SubdomainsData {
    SubdomainsData {
        count: resp.count,
        limit: resp.limit,
        subdomains: resp
            .subdomains
            .into_iter()
            .map(|s| {
                let age = format_age(&s.created_at);
                SubdomainRow {
                    subdomain: s.subdomain,
                    created_at: s.created_at,
                    age,
                }
            })
            .collect(),
    }
}

async fn fetch(config: &Config) -> Result<ApiResponse, Box<dyn std::error::Error>> {
    let base = api_base(config);
    let token = config.auth.access_token.as_deref().unwrap_or_default();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let resp = client
        .get(format!("{base}/api/subdomains"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API error: {body}").into());
    }

    Ok(resp.json().await?)
}

fn fetch_blocking(config: &Config) -> SubdomainsData {
    let base = api_base(config);
    let token = config
        .auth
        .access_token
        .as_deref()
        .unwrap_or_default()
        .to_string();

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok();

    let Some(client) = client else {
        return SubdomainsData {
            subdomains: vec![],
            limit: 0,
            count: 0,
        };
    };

    let resp = client
        .get(format!("{base}/api/subdomains"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .ok();

    let Some(resp) = resp else {
        return SubdomainsData {
            subdomains: vec![],
            limit: 0,
            count: 0,
        };
    };

    match resp.json::<ApiResponse>() {
        Ok(data) => to_tui_data(data),
        Err(_) => SubdomainsData {
            subdomains: vec![],
            limit: 0,
            count: 0,
        },
    }
}

fn delete_blocking(config: &Config, name: &str) -> Result<(), String> {
    let base = api_base(config);
    let token = config
        .auth
        .access_token
        .as_deref()
        .unwrap_or_default()
        .to_string();

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .delete(format!("{base}/api/subdomains/{name}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("API error: {body}"));
    }

    Ok(())
}

pub async fn list() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;
    if !config.is_authenticated() {
        return Err("not logged in - run: xpo login".into());
    }

    let data = fetch(&config).await?;

    if data.limit == 0 {
        println!(
            "  {} Reserved subdomains require Pro plan",
            console::style("i").cyan().bold()
        );
        return Ok(());
    }

    let initial = to_tui_data(data);
    let config_r = config.clone();
    let config_d = config.clone();

    let rt = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        xpo_tui::subdomains_app::run(
            initial,
            move || rt.block_on(async { fetch_blocking(&config_r) }),
            move |name| delete_blocking(&config_d, name),
        )
    })
    .await??;

    Ok(())
}

pub async fn remove(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;
    if !config.is_authenticated() {
        return Err("not logged in - run: xpo login".into());
    }

    let base = api_base(&config);
    let token = config.auth.access_token.as_deref().unwrap_or_default();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let resp = client
        .delete(format!("{base}/api/subdomains/{name}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API error: {body}").into());
    }

    println!(
        "  {} Removed '{}' from reserved subdomains",
        console::style("\u{2713}").green().bold(),
        console::style(name).white().bold()
    );

    Ok(())
}
