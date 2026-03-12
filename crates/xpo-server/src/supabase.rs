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

#[derive(Debug, Deserialize)]
struct ReservedSubdomain {
    user_id: String,
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

    pub async fn get_subdomain_owner(&self, subdomain: &str) -> Result<Option<String>, String> {
        let url = format!(
            "{}/rest/v1/reserved_subdomains?subdomain=eq.{}&select=user_id",
            self.base_url, subdomain
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

        let rows: Vec<ReservedSubdomain> = resp
            .json()
            .await
            .map_err(|e| format!("PostgREST parse failed: {e}"))?;

        Ok(rows.into_iter().next().map(|r| r.user_id))
    }

    pub async fn get_user_reserved_count(&self, user_id: &str) -> Result<usize, String> {
        let url = format!(
            "{}/rest/v1/reserved_subdomains?user_id=eq.{}&select=subdomain",
            self.base_url, user_id
        );

        let resp = self
            .client
            .get(&url)
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .header("Prefer", "count=exact")
            .header("Range-Unit", "items")
            .header("Range", "0-0")
            .send()
            .await
            .map_err(|e| format!("PostgREST query failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("PostgREST returned {}", resp.status()));
        }

        let count = resp
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split('/').next_back())
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);

        Ok(count)
    }

    pub async fn get_user_subdomains(
        &self,
        user_id: &str,
    ) -> Result<Vec<serde_json::Value>, String> {
        let url = format!(
            "{}/rest/v1/reserved_subdomains?user_id=eq.{}&select=subdomain,created_at&order=created_at.asc",
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

        resp.json()
            .await
            .map_err(|e| format!("PostgREST parse failed: {e}"))
    }

    pub async fn delete_subdomain(&self, user_id: &str, subdomain: &str) -> Result<(), String> {
        let url = format!(
            "{}/rest/v1/reserved_subdomains?subdomain=eq.{}&user_id=eq.{}",
            self.base_url, subdomain, user_id
        );

        let resp = self
            .client
            .delete(&url)
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .send()
            .await
            .map_err(|e| format!("PostgREST delete failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("PostgREST returned {}", resp.status()));
        }

        Ok(())
    }

    pub async fn reserve_subdomain(&self, user_id: &str, subdomain: &str) -> Result<(), String> {
        let url = format!("{}/rest/v1/reserved_subdomains", self.base_url);

        let resp = self
            .client
            .post(&url)
            .header("apikey", &self.service_role_key)
            .header("Authorization", format!("Bearer {}", self.service_role_key))
            .header("Prefer", "return=minimal")
            .json(&serde_json::json!({
                "subdomain": subdomain,
                "user_id": user_id,
            }))
            .send()
            .await
            .map_err(|e| format!("PostgREST insert failed: {e}"))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("reserve failed: {body}"));
        }

        Ok(())
    }
}
