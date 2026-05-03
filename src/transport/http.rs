use crate::config::Profile;
use anyhow::Result;
use reqwest::Client;

pub struct HttpClient {
    pub base_url: String,
    pub user: String,
    pub password: Option<String>,
    client: Client,
}

impl HttpClient {
    pub fn new(profile: &Profile) -> Self {
        Self {
            base_url: profile.http_base(),
            user: profile.user.clone(),
            password: profile.password.clone(),
            client: Client::builder()
                .use_rustls_tls()
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    pub fn get(&self, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.get(&url);
        if let Some(pass) = &self.password {
            req = req.basic_auth(&self.user, Some(pass));
        } else {
            req = req.basic_auth(&self.user, None::<&str>);
        }
        req
    }

    pub fn put(&self, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.put(&url);
        if let Some(pass) = &self.password {
            req = req.basic_auth(&self.user, Some(pass));
        } else {
            req = req.basic_auth(&self.user, None::<&str>);
        }
        req
    }

    pub async fn get_json(&self, path: &str) -> Result<serde_json::Value> {
        let resp = self.get(path).send().await?;
        let status = resp.status();
        let body: serde_json::Value = resp.json().await?;
        if !status.is_success() {
            anyhow::bail!("HTTP {} from {}: {}", status, path, body);
        }
        Ok(body)
    }
}
