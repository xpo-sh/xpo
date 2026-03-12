use console::style;
use serde::Deserialize;
use xpo_core::config::Config;

#[derive(Debug, Deserialize)]
struct SubdomainsResponse {
    subdomains: Vec<SubdomainEntry>,
    limit: i32,
    count: usize,
}

#[derive(Debug, Deserialize)]
struct SubdomainEntry {
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
            let dur = now.signed_duration_since(dt);
            return format_duration(dur);
        }
        return String::new();
    };
    let now = chrono::Utc::now();
    let dur = now.signed_duration_since(dt);
    format_duration(dur)
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

async fn fetch(config: &Config) -> Result<SubdomainsResponse, Box<dyn std::error::Error>> {
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

async fn delete(config: &Config, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let base = api_base(config);
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
            style("i").cyan().bold()
        );
        return Ok(());
    }

    println!();
    println!(
        "  {} Reserved Subdomains ({}/{})",
        style("\u{25cf}").cyan(),
        style(data.count).cyan().bold(),
        style(data.limit).dim()
    );
    println!();

    if data.subdomains.is_empty() {
        println!("    {} No reserved subdomains yet", style("\u{2022}").dim());
        println!(
            "    {} Use {} to auto-reserve",
            style("\u{2022}").dim(),
            style("xpo share -s <name>").cyan()
        );
    } else {
        let max_len = data
            .subdomains
            .iter()
            .map(|s| s.subdomain.len())
            .max()
            .unwrap_or(10);
        let col = max_len.max(10);

        for sub in &data.subdomains {
            let age = format_age(&sub.created_at);
            println!(
                "    {}  {:<col$}  {}",
                style("\u{25cf}").green(),
                style(&sub.subdomain).white().bold(),
                style(age).dim(),
                col = col
            );
        }
    }

    println!();
    println!(
        "    {} Remove: {}",
        style("tip").dim(),
        style("xpo subdomains rm <name>").cyan()
    );
    println!();

    Ok(())
}

pub async fn remove(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;
    if !config.is_authenticated() {
        return Err("not logged in - run: xpo login".into());
    }

    delete(&config, name).await?;

    println!(
        "  {} Removed '{}' from reserved subdomains",
        style("\u{2713}").green().bold(),
        style(name).white().bold()
    );

    Ok(())
}
