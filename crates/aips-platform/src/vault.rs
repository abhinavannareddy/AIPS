//! HashiCorp Vault secret loading (KV v2). Supports token or AppRole login.
//! Falls back to environment variables when Vault is not configured (development only).
use std::collections::HashMap;

pub struct Secrets {
    values: HashMap<String, String>,
    pub source: &'static str,
}

impl Secrets {
    /// Secret from Vault, else the environment variable of the same name.
    pub fn get(&self, key: &str) -> Option<String> {
        self.values
            .get(key)
            .cloned()
            .or_else(|| std::env::var(key).ok())
            .filter(|v| !v.is_empty())
    }
}

pub async fn load(path: &str) -> Secrets {
    let Ok(addr) = std::env::var("VAULT_ADDR") else {
        return Secrets {
            values: HashMap::new(),
            source: "environment",
        };
    };
    match fetch(&addr, path).await {
        Ok(values) => {
            tracing::info!(keys = values.len(), path, "secrets loaded from Vault");
            Secrets {
                values,
                source: "vault",
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Vault unavailable; falling back to environment");
            Secrets {
                values: HashMap::new(),
                source: "environment",
            }
        }
    }
}

async fn fetch(addr: &str, path: &str) -> anyhow::Result<HashMap<String, String>> {
    let http = reqwest::Client::new();
    let mount = std::env::var("VAULT_KV_MOUNT").unwrap_or_else(|_| "secret".into());
    let token = match std::env::var("VAULT_TOKEN") {
        Ok(t) => t,
        Err(_) => {
            let role_id = std::env::var("VAULT_ROLE_ID")?;
            let secret_id = std::env::var("VAULT_SECRET_ID")?;
            let v: serde_json::Value = http
                .post(format!("{addr}/v1/auth/approle/login"))
                .json(&serde_json::json!({ "role_id": role_id, "secret_id": secret_id }))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            v["auth"]["client_token"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        }
    };
    let v: serde_json::Value = http
        .get(format!("{addr}/v1/{mount}/data/{path}"))
        .header("X-Vault-Token", token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut out = HashMap::new();
    if let Some(map) = v["data"]["data"].as_object() {
        for (k, val) in map {
            if let Some(s) = val.as_str() {
                out.insert(k.clone(), s.to_string());
            }
        }
    }
    Ok(out)
}
