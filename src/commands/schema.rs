use anyhow::Result;
use clap::Subcommand;
use sqlx::Row;
use crate::output::{Format, ResultSet};
use crate::transport::Connection;

#[derive(Subcommand)]
pub enum SchemaCmd {
    /// List databases
    ListDbs,
    /// List tables in a database
    ListTables {
        #[arg(short, long)]
        db: Option<String>,
    },
    /// Describe a table's columns
    Describe {
        table: String,
        #[arg(short, long)]
        db: Option<String>,
    },
    /// Apply DDL from a file
    Apply {
        #[arg(short = 'f', long)]
        file: String,
        /// Preview without executing
        #[arg(long)]
        dry_run: bool,
    },
    /// Compare local DDL file to live schema
    Diff {
        table: String,
        #[arg(short = 'f', long)]
        file: String,
        #[arg(short, long)]
        db: Option<String>,
    },
}

pub async fn run(cmd: SchemaCmd, conn: &Connection, format: Format) -> Result<()> {
    let pool = conn.mysql().await?;

    match cmd {
        SchemaCmd::ListDbs => {
            let rows = sqlx::query("SHOW DATABASES").fetch_all(&pool).await?;
            let cols = vec!["Database".to_string()];
            let data: Vec<Vec<String>> = rows.iter()
                .map(|r| vec![r.try_get::<String, _>(0).unwrap_or_default()])
                .collect();
            ResultSet::new(cols, data).print(format)?;
        }

        SchemaCmd::ListTables { db } => {
            if let Some(db) = &db {
                sqlx::query(&format!("USE `{}`", db)).execute(&pool).await?;
            } else if let Some(db) = &conn.profile.database {
                sqlx::query(&format!("USE `{}`", db)).execute(&pool).await?;
            }
            let rows = sqlx::query("SHOW TABLES").fetch_all(&pool).await?;
            let cols = vec!["Table".to_string()];
            let data: Vec<Vec<String>> = rows.iter()
                .map(|r| vec![r.try_get::<String, _>(0).unwrap_or_default()])
                .collect();
            ResultSet::new(cols, data).print(format)?;
        }

        SchemaCmd::Describe { table, db } => {
            if let Some(db) = &db {
                sqlx::query(&format!("USE `{}`", db)).execute(&pool).await?;
            } else if let Some(db) = &conn.profile.database {
                sqlx::query(&format!("USE `{}`", db)).execute(&pool).await?;
            }
            let rows = sqlx::query(&format!("DESCRIBE `{}`", table))
                .fetch_all(&pool).await?;
            if rows.is_empty() {
                println!("Table '{}' not found or is empty.", table);
                return Ok(());
            }
            let cols: Vec<String> = rows[0].columns().iter()
                .map(|c| c.name().to_string()).collect();
            let data: Vec<Vec<String>> = rows.iter().map(|row| {
                (0..cols.len()).map(|i| {
                    row.try_get::<String, _>(i).unwrap_or_else(|_| "NULL".to_string())
                }).collect()
            }).collect();
            ResultSet::new(cols, data).print(format)?;
        }

        SchemaCmd::Apply { file, dry_run } => {
            let sql = std::fs::read_to_string(&file)
                .map_err(|e| anyhow::anyhow!("Cannot read '{}': {}", file, e))?;
            // Split on semicolons, filter empty
            let stmts: Vec<&str> = sql.split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            println!("{} statement(s) in '{}'", stmts.len(), file);
            if dry_run {
                for (i, stmt) in stmts.iter().enumerate() {
                    println!("[{}] {}", i + 1, &stmt[..stmt.len().min(120)]);
                }
                println!("(dry-run — nothing executed)");
            } else {
                for (i, stmt) in stmts.iter().enumerate() {
                    sqlx::query(stmt).execute(&pool).await
                        .map_err(|e| anyhow::anyhow!("Statement {} failed: {}\nSQL: {}", i+1, e, stmt))?;
                    println!("[{}] OK", i + 1);
                }
            }
        }

        SchemaCmd::Diff { table, file, db } => {
            if let Some(db) = &db {
                sqlx::query(&format!("USE `{}`", db)).execute(&pool).await?;
            } else if let Some(db) = &conn.profile.database {
                sqlx::query(&format!("USE `{}`", db)).execute(&pool).await?;
            }
            let local_ddl = std::fs::read_to_string(&file)?;
            let row = sqlx::query(&format!("SHOW CREATE TABLE `{}`", table))
                .fetch_one(&pool).await
                .map_err(|_| anyhow::anyhow!("Table '{}' not found", table))?;
            let live_ddl: String = row.try_get(1).unwrap_or_default();
            println!("=== Local ({}) ===", file);
            println!("{}", local_ddl.trim());
            println!("\n=== Live ({}) ===", table);
            println!("{}", live_ddl.trim());
            if local_ddl.trim() == live_ddl.trim() {
                println!("\n✓ No differences.");
            } else {
                println!("\n⚠ Schemas differ (manual review required).");
            }
        }
    }
    Ok(())
}
