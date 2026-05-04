
pub mod mysql;
pub mod http;

use anyhow::Result;
use crate::config::Profile;
use std::sync::Mutex;

pub struct Connection {
    pub profile: Profile,
    pool: Mutex<Option<::mysql::Pool>>,
}

impl Connection {
    pub fn new(profile: Profile) -> Self {
        Self { profile, pool: Mutex::new(None) }
    }

    /// Lazily initialise the MySQL connection pool.
    pub fn mysql_pool(&self) -> Result<::mysql::Pool> {
        let mut guard = self.pool.lock().unwrap();
        if guard.is_none() {
            *guard = Some(mysql::pool(&self.profile)?);
        }
        Ok(guard.as_ref().unwrap().clone())
    }

    /// Run a SELECT/SHOW/DESCRIBE query and return a result set.
    pub fn query(&self, sql: &str) -> Result<mysql::QueryResult> {
        tracing::debug!("mysql query: {}", sql);
        mysql::query(&self.mysql_pool()?, sql)
    }

    /// Run a DDL/DML statement.
    pub fn execute(&self, sql: &str) -> Result<u64> {
        tracing::debug!("mysql execute: {}", sql);
        mysql::execute(&self.mysql_pool()?, sql)
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
