
//! HTTP/REST transport using ureq (sync, rustls, no OpenSSL).

use anyhow::{Context, Result};
use crate::config::Profile;

pub struct HttpClient {
    pub base_url: String,
    pub user: String,
    pub password: Option<String>,
    agent: ureq::Agent,
}

impl HttpClient {
    pub fn new(profile: &Profile) -> Self {
        Self {
            base_url: profile.http_base(),
            user: profile.user.clone(),
            password: profile.password.clone(),
            agent: ureq::AgentBuilder::new()
                .redirects(3)
                .build(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn auth(&self, req: ureq::Request) -> ureq::Request {
        req.set(
            "Authorization",
            &basic_auth(&self.user, self.password.as_deref().unwrap_or("")),
        )
    }

    pub fn get_json(&self, path: &str) -> Result<serde_json::Value> {
        tracing::debug!("http GET {}", path);
        let resp = self
            .auth(self.agent.get(&self.url(path)))
            .call()
            .with_context(|| format!("GET {} failed", path))?;
        Ok(resp.into_json()?)
    }

    pub fn put_bytes(
        &self,
        path: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> Result<serde_json::Value> {
        tracing::debug!("http PUT {} ({} bytes)", path, body.len());
        let mut req = self.auth(self.agent.put(&self.url(path)));
        for (k, v) in headers {
            req = req.set(k, v);
        }
        let resp = req
            .send_bytes(&body)
            .with_context(|| format!("PUT {} failed", path))?;
        Ok(resp.into_json()?)
    }
}

fn basic_auth(user: &str, pass: &str) -> String {
    let encoded = {
        let input = format!("{}:{}", user, pass);
        base64_encode(input.as_bytes())
    };
    format!("Basic {}", encoded)
}

fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        out.push(CHARS[b0 >> 2] as char);
        out.push(CHARS[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 { out.push(CHARS[((b1 & 15) << 2) | (b2 >> 6)] as char); } else { out.push('='); }
        if chunk.len() > 2 { out.push(CHARS[b2 & 63] as char); } else { out.push('='); }
    }
    out
}