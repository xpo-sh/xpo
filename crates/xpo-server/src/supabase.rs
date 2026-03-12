use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

const POSTGREST_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct UserProfile {
    pub id: String,
    pub plan: String,
    pub max_tunnels: i32,
    pub max_ttl_secs: Option<i32>,
    pub max_reserved_subdomains: i32,
}

#[derive(Clone)]
pub struct SupabaseClient {
    client: Client,
    base_url: String,
    service_role_key: String,
}

impl SupabaseClient {
    pub fn new(base_url: String, service_role_key: String) -> Self {
        let client = Client::builder()
            .timeout(POSTGREST_TIMEOUT)
            .build()
            .expect("failed to build HTTP client");

        Self {
            client,
            base_url,
            service_role_key,
        }
    }

    pub async fn get_user_profile(&self, user_id: &str) -> Result<Option<UserProfile>, String> {
        let url = format!(
            "{}/rest/v1/user_profiles?id=eq.{}&select=*",
            self.base_url, user_id
        );

        let resp = self
            .client
            .get(&url)
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .send()
            .await
            .map_err(|e| format!("PostgREST query failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("PostgREST returned {}", resp.status()));
        }

        let profiles: Vec<UserProfile> = resp
            .json()
            .await
            .map_err(|e| format!("PostgREST parse failed: {e}"))?;

        Ok(profiles.into_iter().next())
    }
}
