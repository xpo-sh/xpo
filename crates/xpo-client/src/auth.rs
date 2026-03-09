use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use console::style;
use sha2::{Digest, Sha256};
use xpo_core::config::Config;

const DEFAULT_AUTH_URL: &str = "https://auth.xpo.sh";

fn auth_url() -> String {
    std::env::var("XPO_AUTH_URL").unwrap_or_else(|_| DEFAULT_AUTH_URL.to_string())
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn random_alphanumeric(len: usize) -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    (0..len)
        .map(|_| {
            let idx = rng.random_range(0..62u8);
            match idx {
                0..=9 => (b'0' + idx) as char,
                10..=35 => (b'a' + idx - 10) as char,
                _ => (b'A' + idx - 36) as char,
            }
        })
        .collect()
}

fn generate_pkce() -> (String, String) {
    let verifier = random_alphanumeric(64);
    let hash = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hash);
    (verifier, challenge)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                result.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            result.push(b' ');
        } else {
            result.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn();
    }
}

pub async fn login(provider: &str) -> Result<(), Box<dyn std::error::Error>> {
    let auth = auth_url();
    let (code_verifier, code_challenge) = generate_pkce();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:9876").await?;

    let nonce = random_alphanumeric(8);
    let url = format!(
        "{}/authorize?provider={}&redirect_to=http://127.0.0.1:9876/callback&code_challenge={}&code_challenge_method=S256&t={}",
        auth, provider, code_challenge, nonce
    );

    println!(
        "  {} Opening browser for {} login...",
        style("→").cyan().bold(),
        provider
    );
    open_browser(&url);
    println!("  {} Waiting for authentication...", style("⏳").dim());

    let (mut stream, _) = listener.accept().await?;

    let mut buf = vec![0u8; 4096];
    let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let first_line = request.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("");

    let code = extract_query_param(path, "code")
        .ok_or("no auth code received - authentication may have failed")?;
    let code = percent_decode(&code);

    let html = r#"<!DOCTYPE html><html><body style="font-family:monospace;display:flex;align-items:center;justify-content:center;height:100vh;background:#0a0a0f;color:#e2e2e8"><div style="text-align:center"><h2 style="color:#22d3ee">xpo</h2><p>Login successful! You can close this tab.</p></div></body></html>"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await?;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/token?grant_type=pkce", auth))
        .json(&serde_json::json!({
            "auth_code": code,
            "code_verifier": code_verifier,
        }))
        .send()
        .await?;

    let status = resp.status();
    let body: serde_json::Value = resp.json().await?;

    if !status.is_success() {
        let msg = body["error_description"]
            .as_str()
            .or(body["msg"].as_str())
            .unwrap_or("authentication failed");
        return Err(msg.into());
    }

    let access_token = body["access_token"]
        .as_str()
        .ok_or("no access_token in response")?;
    let refresh_token = body["refresh_token"]
        .as_str()
        .ok_or("no refresh_token in response")?;
    let expires_in = body["expires_in"].as_u64().unwrap_or(3600);

    let user = &body["user"];
    let user_id = user["id"].as_str().map(String::from);
    let email = user["email"].as_str().map(String::from);

    let mut config = Config::load().unwrap_or_default();
    config.access_token = Some(access_token.to_string());
    config.refresh_token = Some(refresh_token.to_string());
    config.expires_at = Some(now() + expires_in);
    config.user_id = user_id;
    config.email = email.clone();
    config.save()?;

    println!(
        "  {} Logged in as {}",
        style("✓").green().bold(),
        style(email.as_deref().unwrap_or("user")).cyan()
    );

    Ok(())
}

pub async fn refresh_token() -> Result<String, Box<dyn std::error::Error>> {
    let config = Config::load()?;
    let refresh = config
        .refresh_token
        .as_deref()
        .ok_or("no refresh token - run xpo login")?;

    let auth = auth_url();
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/token?grant_type=refresh_token", auth))
        .json(&serde_json::json!({
            "refresh_token": refresh,
        }))
        .send()
        .await?;

    let status = resp.status();
    let body: serde_json::Value = resp.json().await?;

    if !status.is_success() {
        let msg = body["error_description"]
            .as_str()
            .unwrap_or("token refresh failed");
        return Err(msg.into());
    }

    let access_token = body["access_token"]
        .as_str()
        .ok_or("no access_token")?
        .to_string();
    let new_refresh = body["refresh_token"].as_str().ok_or("no refresh_token")?;
    let expires_in = body["expires_in"].as_u64().unwrap_or(3600);

    let mut config = Config::load().unwrap_or_default();
    config.access_token = Some(access_token.clone());
    config.refresh_token = Some(new_refresh.to_string());
    config.expires_at = Some(now() + expires_in);
    config.save()?;

    Ok(access_token)
}

pub async fn get_token() -> Result<String, Box<dyn std::error::Error>> {
    let config = Config::load().unwrap_or_default();

    if !config.is_authenticated() {
        return Err("not logged in - run: xpo login".into());
    }

    if config.is_expired() {
        return refresh_token().await;
    }

    Ok(config.access_token.unwrap())
}

fn extract_query_param(path: &str, key: &str) -> Option<String> {
    let query = path.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next()? == key {
            return parts.next().map(|v| v.to_string());
        }
    }
    None
}
