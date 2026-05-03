
//! Apache Iceberg integration (feature = "iceberg").
//!
//! Doris 2.x supports Iceberg catalogs natively via `CREATE CATALOG … TYPE = iceberg`.
//! These commands query catalog metadata through the MySQL transport (SHOW CATALOGS,
//! SHOW SNAPSHOTS, etc.) and optionally call a REST catalog endpoint directly.

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use crate::output::{Format, ResultSet};
use crate::transport::Connection;

#[derive(Subcommand)]
pub enum IcebergCmd {
    /// List all Iceberg catalogs registered in Doris
    Catalogs,
    /// List databases inside an Iceberg catalog
    Databases {
        catalog: String,
    },
    /// List tables inside an Iceberg catalog database
    Tables {
        catalog: String,
        database: String,
    },
    /// Show snapshots for an Iceberg table
    Snapshots {
        /// Fully-qualified name: catalog.database.table
        table: String,
    },
    /// Show partitions for an Iceberg table
    Partitions {
        table: String,
    },
    /// Display the schema of an Iceberg table
    Schema {
        table: String,
    },
    /// Time-travel query: SELECT * FROM table FOR VERSION AS OF <snapshot_id>
    TimeTravel {
        table: String,
        #[arg(long)]
        snapshot_id: i64,
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[arg(short, long, default_value = "table")]
        format: Format,
    },
}

pub async fn run(cmd: IcebergCmd, conn: &Connection, format: Format) -> Result<()> {
    match cmd {
        IcebergCmd::Catalogs => {
            let r = conn.query("SHOW CATALOGS")?;
            // Filter to Iceberg catalogs only
            let iceberg_col = r.columns.iter().position(|c| c.to_lowercase().contains("type"));
            let rows: Vec<Vec<String>> = if let Some(type_idx) = iceberg_col {
                r.rows.into_iter()
                    .filter(|row| row.get(type_idx).map(|t| t.to_lowercase().contains("iceberg")).unwrap_or(false))
                    .collect()
            } else {
                r.rows
            };
            if rows.is_empty() {
                println!("No Iceberg catalogs registered. Use `CREATE CATALOG` in Doris to add one.");
            } else {
                ResultSet::new(r.columns, rows).print(format)?;
            }
        }

        IcebergCmd::Databases { catalog } => {
            let r = conn.query(&format!("SHOW DATABASES FROM `{}`", catalog))?;
            ResultSet::new(r.columns, r.rows).print(format)?;
        }

        IcebergCmd::Tables { catalog, database } => {
            let r = conn.query(&format!("SHOW TABLES FROM `{}`.`{}`", catalog, database))?;
            ResultSet::new(r.columns, r.rows).print(format)?;
        }

        IcebergCmd::Snapshots { table } => {
            let sql = format!(
                "SELECT * FROM iceberg_meta('table' = '{}', 'query_type' = 'snapshots')",
                table
            );
            match conn.query(&sql) {
                Ok(r) => ResultSet::new(r.columns, r.rows).print(format)?,
                Err(e) => {
                    // Fallback: some Doris builds use a different syntax
                    eprintln!("{} iceberg_meta() failed: {}", "warn:".yellow(), e);
                    let r = conn.query(&format!("SHOW PARTITIONS FROM {}", table))?;
                    ResultSet::new(r.columns, r.rows).print(format)?;
                }
            }
        }

        IcebergCmd::Partitions { table } => {
            let sql = format!(
                "SELECT * FROM iceberg_meta('table' = '{}', 'query_type' = 'partitions')",
                table
            );
            let r = conn.query(&sql)?;
            ResultSet::new(r.columns, r.rows).print(format)?;
        }

        IcebergCmd::Schema { table } => {
            let sql = format!(
                "SELECT * FROM iceberg_meta('table' = '{}', 'query_type' = 'schema')",
                table
            );
            match conn.query(&sql) {
                Ok(r) => ResultSet::new(r.columns, r.rows).print(format)?,
                Err(_) => {
                    // Fallback: DESCRIBE works too if the table is loaded into Doris session
                    let r = conn.query(&format!("DESCRIBE {}", table))?;
                    ResultSet::new(r.columns, r.rows).print(format)?;
                }
            }
        }

        IcebergCmd::TimeTravel { table, snapshot_id, limit, format } => {
            let sql = format!(
                "SELECT * FROM {} FOR VERSION AS OF {} LIMIT {}",
                table, snapshot_id, limit
            );
            println!("{}", format!("-- {}", sql).dimmed());
            let r = conn.query(&sql)?;
            ResultSet::new(r.columns, r.rows).print(format)?;
        }
    }
    Ok(())
}