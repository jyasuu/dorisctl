pub mod mysql;
pub mod http;

use crate::config::Profile;

/// Wraps both MySQL and HTTP transports for a single profile
pub struct Connection {
    pub profile: Profile,
}

impl Connection {
    pub fn new(profile: Profile) -> Self {
        Self { profile }
    }

    pub async fn mysql(&self) -> anyhow::Result<sqlx::MySqlPool> {
        mysql::connect(&self.profile).await
    }

    pub fn http(&self) -> http::HttpClient {
        http::HttpClient::new(&self.profile)
    }
}
