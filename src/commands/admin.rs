use anyhow::Result;
use clap::Subcommand;
use crate::output::{Format, ResultSet};
use crate::transport::Connection;

#[derive(Subcommand)]
pub enum AdminCmd {
    /// Show backend nodes
    Backends,
    /// Show frontend nodes
    Frontends,
    /// Show tablets for a table
    Tablets {
        #[arg(long)]
        table: String,
        #[arg(long)]
        db: Option<String>,
    },
    /// Manage load jobs
    Jobs {
        #[command(subcommand)]
        action: JobsCmd,
    },
    /// Get or set FE configuration
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
}

#[derive(Subcommand)]
pub enum JobsCmd {
    /// List load jobs
    List {
        #[arg(long)]
        db: Option<String>,
    },
    /// Pause a routine load job
    Pause { id: String },
    /// Resume a routine load job
    Resume { id: String },
    /// Cancel a routine load job
    Cancel { id: String },
}

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Get a configuration value
    Get { key: String },
    /// Set a configuration value
    Set { key: String, value: String },
}

pub async fn run(cmd: AdminCmd, conn: &Connection, format: Format) -> Result<()> {
    match cmd {
        AdminCmd::Backends => {
            let body = conn.http().get_json("/api/backends").await?;
            print_backends(&body, format)?;
        }
        AdminCmd::Frontends => {
            let pool = conn.mysql().await?;
            let rows = sqlx::query("SHOW FRONTENDS").fetch_all(&pool).await?;
            if rows.is_empty() {
                println!("(no frontends)");
                return Ok(());
            }
            use sqlx::Row;
            let cols: Vec<String> = rows[0].columns().iter().map(|c| c.name().to_string()).collect();
            let data: Vec<Vec<String>> = rows.iter().map(|row| {
                (0..cols.len()).map(|i| {
                    row.try_get::<String, _>(i).unwrap_or_else(|_| "NULL".to_string())
                }).collect()
            }).collect();
            ResultSet::new(cols, data).print(format)?;
        }
        AdminCmd::Tablets { table, db } => {
            let pool = conn.mysql().await?;
            if let Some(db) = &db {
                sqlx::query(&format!("USE `{}`", db)).execute(&pool).await?;
            } else if let Some(db) = &conn.profile.database {
                sqlx::query(&format!("USE `{}`", db)).execute(&pool).await?;
            }
            let rows = sqlx::query(&format!("SHOW TABLETS FROM `{}`", table))
                .fetch_all(&pool).await?;
            if rows.is_empty() {
                println!("(no tablets)");
                return Ok(());
            }
            use sqlx::Row;
            let cols: Vec<String> = rows[0].columns().iter().map(|c| c.name().to_string()).collect();
            let data: Vec<Vec<String>> = rows.iter().map(|row| {
                (0..cols.len()).map(|i| {
                    row.try_get::<String, _>(i).unwrap_or_else(|_| "NULL".to_string())
                }).collect()
            }).collect();
            ResultSet::new(cols, data).print(format)?;
        }
        AdminCmd::Jobs { action } => {
            let pool = conn.mysql().await?;
            match action {
                JobsCmd::List { db } => {
                    if let Some(db) = &db {
                        sqlx::query(&format!("USE `{}`", db)).execute(&pool).await?;
                    }
                    let rows = sqlx::query("SHOW ROUTINE LOAD").fetch_all(&pool).await?;
                    if rows.is_empty() {
                        println!("(no routine load jobs)");
                        return Ok(());
                    }
                    use sqlx::Row;
                    let cols: Vec<String> = rows[0].columns().iter().map(|c| c.name().to_string()).collect();
                    let data: Vec<Vec<String>> = rows.iter().map(|row| {
                        (0..cols.len()).map(|i| {
                            row.try_get::<String, _>(i).unwrap_or_else(|_| "NULL".to_string())
                        }).collect()
                    }).collect();
                    ResultSet::new(cols, data).print(format)?;
                }
                JobsCmd::Pause { id } => {
                    sqlx::query(&format!("PAUSE ROUTINE LOAD FOR {}", id)).execute(&pool).await?;
                    println!("Job {} paused.", id);
                }
                JobsCmd::Resume { id } => {
                    sqlx::query(&format!("RESUME ROUTINE LOAD FOR {}", id)).execute(&pool).await?;
                    println!("Job {} resumed.", id);
                }
                JobsCmd::Cancel { id } => {
                    sqlx::query(&format!("STOP ROUTINE LOAD FOR {}", id)).execute(&pool).await?;
                    println!("Job {} cancelled.", id);
                }
            }
        }
        AdminCmd::Config { action } => {
            match action {
                ConfigCmd::Get { key } => {
                    let body = conn.http()
                        .get_json(&format!("/api/_get_config?conf_item={}", key))
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&body)?);
                }
                ConfigCmd::Set { key, value } => {
                    let body = conn.http()
                        .get_json(&format!("/api/_set_config?{}={}", key, value))
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&body)?);
                }
            }
        }
    }
    Ok(())
}

fn print_backends(body: &serde_json::Value, format: Format) -> Result<()> {
    // Doris returns backends as an object keyed by backend address
    let backends = match body.as_object() {
        Some(obj) => obj,
        None => {
            println!("{}", serde_json::to_string_pretty(body)?);
            return Ok(());
        }
    };

    let cols = vec!["Host".to_string(), "Alive".to_string(), "TabletNum".to_string(), "DataUsedCapacity".to_string(), "TotalCapacity".to_string()];
    let mut rows = Vec::new();
    for (host, info) in backends {
        rows.push(vec![
            host.clone(),
            info["isAlive"].as_str().unwrap_or("").to_string(),
            info["tabletNum"].to_string(),
            info["dataUsedCapacity"].as_str().unwrap_or("0").to_string(),
            info["totalCapacity"].as_str().unwrap_or("0").to_string(),
        ]);
    }
    ResultSet::new(cols, rows).print(format)?;
    Ok(())
}
