use crate::config::ServerConfig;
use crate::tls::CertResolver;
use instant_acme::{
    Account, AccountCredentials, ChallengeType, Identifier, LetsEncrypt, NewAccount, NewOrder,
    RetryPolicy,
};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

const RENEWAL_CHECK_INTERVAL: Duration = Duration::from_secs(12 * 3600);
const CERT_RENEWAL_AGE: Duration = Duration::from_secs(60 * 24 * 3600);

pub async fn provision_cert(config: &ServerConfig) -> Result<(), Box<dyn std::error::Error>> {
    let email = config
        .acme_email
        .as_deref()
        .ok_or("ACME_EMAIL is required")?;
    let cf_token = config
        .cf_api_token
        .as_deref()
        .ok_or("CF_API_TOKEN is required")?;
    let cf_zone = config
        .cf_zone_id
        .as_deref()
        .ok_or("CF_ZONE_ID is required")?;

    let directory_url = if config.acme_staging {
        LetsEncrypt::Staging.url()
    } else {
        LetsEncrypt::Production.url()
    };

    let account = load_or_create_account(email, directory_url, &config.acme_account_path).await?;

    let domain = format!("*.{}", config.base_domain);
    info!(domain = %domain, staging = config.acme_staging, "requesting certificate");

    let identifiers = vec![Identifier::Dns(domain)];
    let mut order = account.new_order(&NewOrder::new(&identifiers)).await?;

    let txt_name = format!("_acme-challenge.{}", config.base_domain);
    let mut txt_record_ids: Vec<String> = Vec::new();

    {
        let mut auths = order.authorizations();
        while let Some(auth_result) = auths.next().await {
            let mut auth = auth_result?;
            let mut challenge = auth
                .challenge(ChallengeType::Dns01)
                .ok_or("no DNS-01 challenge found")?;

            let key_auth = challenge.key_authorization();
            let txt_value = key_auth.dns_value();

            info!(name = %txt_name, "setting DNS TXT record");
            let record_id = cf_create_txt(cf_token, cf_zone, &txt_name, &txt_value).await?;
            txt_record_ids.push(record_id);

            info!("waiting for DNS propagation");
            tokio::time::sleep(Duration::from_secs(15)).await;

            challenge.set_ready().await?;
        }
    }

    info!("waiting for order to become ready");
    order
        .poll_ready(&RetryPolicy::default().timeout(Duration::from_secs(120)))
        .await?;

    info!("finalizing order");
    let key_pem = order.finalize().await?;

    info!("polling for certificate");
    let cert_pem = order
        .poll_certificate(&RetryPolicy::default().timeout(Duration::from_secs(120)))
        .await?;

    for record_id in &txt_record_ids {
        if let Err(e) = cf_delete_txt(cf_token, cf_zone, record_id).await {
            warn!(record_id = %record_id, "failed to cleanup TXT record: {e}");
        }
    }

    std::fs::create_dir_all(&config.cert_dir)?;
    std::fs::write(config.cert_path(), &cert_pem)?;
    std::fs::write(config.key_path(), &key_pem)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(config.key_path(), std::fs::Permissions::from_mode(0o600))?;
    }

    info!(
        cert = %config.cert_path(),
        key = %config.key_path(),
        "certificate saved"
    );

    Ok(())
}

pub fn spawn_renewal_task(config: Arc<ServerConfig>, resolver: Arc<CertResolver>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(RENEWAL_CHECK_INTERVAL).await;

            if !needs_renewal(&config.cert_path()) {
                continue;
            }

            info!("certificate renewal needed");
            match provision_cert(&config).await {
                Ok(()) => {
                    let certs = crate::tls::load_certs(&config.cert_path());
                    let key = crate::tls::load_key(&config.key_path());
                    resolver.update(certs, key);
                    info!("certificate renewed and hot-reloaded");
                }
                Err(e) => error!("certificate renewal failed: {e}"),
            }
        }
    });
}

async fn load_or_create_account(
    email: &str,
    directory_url: &str,
    account_path: &str,
) -> Result<Account, Box<dyn std::error::Error>> {
    if let Ok(json) = std::fs::read_to_string(account_path) {
        let credentials: AccountCredentials = serde_json::from_str(&json)?;
        let account = Account::builder()?.from_credentials(credentials).await?;
        info!("loaded existing ACME account");
        return Ok(account);
    }

    info!(email = %email, "creating new ACME account");
    let (account, credentials) = Account::builder()?
        .create(
            &NewAccount {
                contact: &[&format!("mailto:{email}")],
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            directory_url.to_string(),
            None,
        )
        .await?;

    if let Some(parent) = std::path::Path::new(account_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&credentials)?;
    std::fs::write(account_path, &json)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(account_path, std::fs::Permissions::from_mode(0o600))?;
    }

    info!("ACME account created and saved");
    Ok(account)
}

async fn cf_create_txt(
    api_token: &str,
    zone_id: &str,
    name: &str,
    content: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url = format!("https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records");

    let resp = client
        .post(&url)
        .bearer_auth(api_token)
        .json(&serde_json::json!({
            "type": "TXT",
            "name": name,
            "content": content,
            "ttl": 60
        }))
        .send()
        .await?;

    let body: serde_json::Value = resp.json().await?;

    if body["success"].as_bool() != Some(true) {
        return Err(format!("Cloudflare API error: {}", body["errors"]).into());
    }

    let record_id = body["result"]["id"]
        .as_str()
        .ok_or("missing record id")?
        .to_string();

    info!(record_id = %record_id, "TXT record created");
    Ok(record_id)
}

async fn cf_delete_txt(
    api_token: &str,
    zone_id: &str,
    record_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let url =
        format!("https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records/{record_id}");

    let resp = client.delete(&url).bearer_auth(api_token).send().await?;
    let body: serde_json::Value = resp.json().await?;

    if body["success"].as_bool() != Some(true) {
        return Err(format!("Cloudflare API error: {}", body["errors"]).into());
    }

    info!(record_id = %record_id, "TXT record deleted");
    Ok(())
}

fn needs_renewal(cert_path: &str) -> bool {
    let metadata = match std::fs::metadata(cert_path) {
        Ok(m) => m,
        Err(_) => return true,
    };
    let modified = match metadata.modified() {
        Ok(t) => t,
        Err(_) => return true,
    };
    let age = std::time::SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    age > CERT_RENEWAL_AGE
}
