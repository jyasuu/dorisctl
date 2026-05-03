use anyhow::Result;
use sqlx::{mysql::MySqlPoolOptions, MySqlPool};
use crate::config::Profile;

pub async fn connect(profile: &Profile) -> Result<MySqlPool> {
    let url = profile.mysql_url();
    let pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .map_err(|e| anyhow::anyhow!(
            "Failed to connect to Doris at {}:{} — {}\nHint: check your profile with `dorisctl profile list`",
            profile.fe_host, profile.mysql_port, e
        ))?;
    Ok(pool)
}
