
pub mod mysql;
pub mod http;

use anyhow::Result;
use crate::config::Profile;

pub struct Connection {
    pub profile: Profile,
    pool: std::sync::OnceLock<::mysql::Pool>,
}

impl Connection {
    pub fn new(profile: Profile) -> Self {
        Self { profile, pool: std::sync::OnceLock::new() }
    }

    /// Lazily initialise the MySQL connection pool.
    pub fn mysql_pool(&self) -> Result<&::mysql::Pool> {
        self.pool.get_or_try_init(|| mysql::pool(&self.profile))
    }

    /// Run a SELECT/SHOW/DESCRIBE query and return a result set.
    pub fn query(&self, sql: &str) -> Result<mysql::QueryResult> {
        tracing::debug!("mysql query: {}", sql);
        mysql::query(self.mysql_pool()?, sql)
    }

    /// Run a DDL/DML statement.
    pub fn execute(&self, sql: &str) -> Result<u64> {
        tracing::debug!("mysql execute: {}", sql);
        mysql::execute(self.mysql_pool()?, sql)
    }

    /// Use a database for subsequent queries.
    pub fn use_db(&self, db: &str) -> Result<()> {
        self.execute(&format!("USE `{}`", db))?;
        Ok(())
    }

    pub fn http(&self) -> http::HttpClient {
        http::HttpClient::new(&self.profile)
    }
}