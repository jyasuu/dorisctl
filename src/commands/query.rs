
use anyhow::{bail, Result};
use clap::Args;
use crate::output::{Format, ResultSet};
use crate::transport::Connection;

#[derive(Args)]
pub struct QueryArgs {
    /// SQL query string
    pub sql: Option<String>,
    /// Read query from a file
    #[arg(long, value_name = "FILE")]
    pub file: Option<String>,
    /// Database to USE before running the query
    #[arg(short, long)]
    pub database: Option<String>,
    /// Preview the query without executing (print SQL only)
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn run(args: QueryArgs, conn: &Connection, format: Format) -> Result<()> {
    let sql = resolve_sql(args.sql, args.file)?;

    // Switch database if requested
    if let Some(db) = &args.database {
        conn.use_db(db)?;
    } else if let Some(db) = &conn.profile.database {
        conn.use_db(db)?;
    }

    if args.dry_run {
        println!("-- dry-run (not executed) --");
        println!("{}", sql);
        return Ok(());
    }

    // Detect whether this is a read (SELECT/SHOW/DESCRIBE/EXPLAIN) or write
    let trimmed = sql.trim_start().to_ascii_uppercase();
    let is_read = trimmed.starts_with("SELECT")
        || trimmed.starts_with("SHOW")
        || trimmed.starts_with("DESCRIBE")
        || trimmed.starts_with("DESC")
        || trimmed.starts_with("EXPLAIN")
        || trimmed.starts_with("WITH");

    if is_read {
        let result = conn.query(&sql)?;
        ResultSet::new(result.columns, result.rows).print(format)?;
    } else {
        let affected = conn.execute(&sql)?;
        println!("OK — {} row(s) affected.", affected);
    }

    Ok(())
}

pub fn resolve_sql(sql: Option<String>, file: Option<String>) -> Result<String> {
    match (sql, file) {
        (Some(s), None) => Ok(s),
        (None, Some(f)) => {
            Ok(std::fs::read_to_string(&f)
                .map_err(|e| anyhow::anyhow!("Cannot read '{}': {}", f, e))?)
        }
        (Some(_), Some(_)) => bail!("Provide either a SQL string or --file, not both."),
        (None, None) => bail!("Provide a SQL string or --file <path>."),
    }
}