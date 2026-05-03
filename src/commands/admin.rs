
use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use crate::output::{Format, ResultSet};
use crate::transport::Connection;

#[derive(Subcommand)]
pub enum AdminCmd {
    /// List backend nodes
    Backends,
    /// List frontend nodes
    Frontends,
    /// Show tablet distribution for a table
    Tablets {
        table: String,
        #[arg(short, long)]
        db: Option<String>,
    },
    /// Manage load / routine-load jobs
    Jobs {
        #[command(subcommand)]
        action: JobsCmd,
    },
    /// Get or set FE configuration
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// Show current warehouse / resource group utilization
    Warehouses,
    /// Compact tablets for a table
    Compact {
        table: String,
        #[arg(short, long)]
        db: Option<String>,
        /// Preview without executing
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub enum JobsCmd {
    /// List routine load jobs
    List {
        #[arg(long)]
        db: Option<String>,
    },
    /// Pause a routine load job
    Pause { name: String },
    /// Resume a routine load job
    Resume { name: String },
    /// Stop/cancel a routine load job
    Cancel { name: String },
}

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Retrieve FE config values
    Get {
        /// Config key (empty = all)
        key: Option<String>,
    },
    /// Set a FE config value at runtime
    Set {
        key: String,
        value: String,
        /// Preview without applying
        #[arg(long)]
        dry_run: bool,
    },
}

pub async fn run(cmd: AdminCmd, conn: &Connection, format: Format) -> Result<()> {
    match cmd {
        AdminCmd::Backends => {
            let body = conn.http().get_json("/api/backends")?;
            print_backends_json(&body, format)?;
        }

        AdminCmd::Frontends => {
            let r = conn.query("SHOW FRONTENDS")?;
            ResultSet::new(r.columns, r.rows).print(format)?;
        }

        AdminCmd::Tablets { table, db } => {
            if let Some(d) = db.as_deref().or(conn.profile.database.as_deref()) {
                conn.use_db(d)?;
            }
            let r = conn.query(&format!("SHOW TABLETS FROM `{}`", table))?;
            if r.rows.is_empty() {
                println!("No tablets found for '{}'.", table);
            } else {
                ResultSet::new(r.columns, r.rows).print(format)?;
            }
        }

        AdminCmd::Jobs { action } => {
            match action {
                JobsCmd::List { db } => {
                    if let Some(d) = db.as_deref() { conn.use_db(d)?; }
                    let r = conn.query("SHOW ROUTINE LOAD")?;
                    if r.rows.is_empty() {
                        println!("No routine load jobs found.");
                    } else {
                        ResultSet::new(r.columns, r.rows).print(format)?;
                    }
                }
                JobsCmd::Pause { name } => {
                    conn.execute(&format!("PAUSE ROUTINE LOAD FOR `{}`", name))?;
                    println!("{} Job '{}' paused.", "✓".green(), name);
                }
                JobsCmd::Resume { name } => {
                    conn.execute(&format!("RESUME ROUTINE LOAD FOR `{}`", name))?;
                    println!("{} Job '{}' resumed.", "✓".green(), name);
                }
                JobsCmd::Cancel { name } => {
                    conn.execute(&format!("STOP ROUTINE LOAD FOR `{}`", name))?;
                    println!("{} Job '{}' cancelled.", "✓".green(), name);
                }
            }
        }

        AdminCmd::Config { action } => {
            match action {
                ConfigCmd::Get { key } => {
                    let path = match &key {
                        Some(k) => format!("/api/_get_config?conf_item={}", k),
                        None    => "/api/_get_config".to_string(),
                    };
                    let body = conn.http().get_json(&path)?;
                    println!("{}", serde_json::to_string_pretty(&body)?);
                }
                ConfigCmd::Set { key, value, dry_run } => {
                    if dry_run {
                        println!("{}", format!("-- dry-run: would set {} = {}", key, value).yellow());
                        return Ok(());
                    }
                    let path = format!("/api/_set_config?{}={}", key, value);
                    let body = conn.http().get_json(&path)?;
                    println!("{}", serde_json::to_string_pretty(&body)?);
                }
            }
        }

        AdminCmd::Warehouses => {
            // Doris SELECT on information_schema
            let r = conn.query(
                "SELECT WAREHOUSE_NAME, STATE, CLUSTER_COUNT, NODE_COUNT \
                 FROM information_schema.warehouses"
            ).unwrap_or_else(|_| {
                // Older Doris may not have this view
                crate::transport::mysql::QueryResult {
                    columns: vec!["info".into()],
                    rows: vec![vec!["Warehouses view not available on this Doris version.".into()]],
                    rows_affected: 0,
                }
            });
            ResultSet::new(r.columns, r.rows).print(format)?;
        }

        AdminCmd::Compact { table, db, dry_run } => {
            if let Some(d) = db.as_deref().or(conn.profile.database.as_deref()) {
                conn.use_db(d)?;
            }
            let sql = format!("ALTER TABLE `{}` COMPACT", table);
            if dry_run {
                println!("{}", format!("-- dry-run: {}", sql).yellow());
                return Ok(());
            }
            conn.execute(&sql)?;
            println!("{} Compaction triggered for '{}'.", "✓".green(), table);
        }
    }
    Ok(())
}

fn print_backends_json(body: &serde_json::Value, format: Format) -> Result<()> {
    // /api/backends returns a map of "host:port" -> {...}
    if let Some(obj) = body.as_object() {
        let cols = vec![
            "Host".to_string(), "IsAlive".to_string(),
            "TabletNum".to_string(), "UsedCapacity".to_string(), "TotalCapacity".to_string(),
        ];
        let rows: Vec<Vec<String>> = obj.iter().map(|(host, info)| vec![
            host.clone(),
            info["isAlive"].as_str().unwrap_or("").to_string(),
            info["tabletNum"].to_string(),
            info["dataUsedCapacity"].as_str().unwrap_or("0").to_string(),
            info["totalCapacity"].as_str().unwrap_or("0").to_string(),
        ]).collect();
        ResultSet::new(cols, rows).print(format)?;
    } else {
        println!("{}", serde_json::to_string_pretty(body)?);
    }
    Ok(())
}