use anyhow::Result;
use clap::Args;
use sqlx::Row;
use crate::output::{Format, ResultSet};
use crate::transport::Connection;

#[derive(Args)]
pub struct QueryArgs {
    /// SQL query string
    pub sql: Option<String>,
    /// Read query from file
    #[arg(short = 'f', long)]
    pub file: Option<String>,
    /// Database to use
    #[arg(short, long)]
    pub database: Option<String>,
}

pub async fn run(args: QueryArgs, conn: &Connection, format: Format) -> Result<()> {
    let sql = if let Some(f) = args.file {
        std::fs::read_to_string(&f)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{}': {}", f, e))?
    } else if let Some(s) = args.sql {
        s
    } else {
        anyhow::bail!("Provide a SQL string or --file <path>");
    };

    let pool = conn.mysql().await?;

    // Switch database if requested
    if let Some(db) = &args.database {
        sqlx::query(&format!("USE `{}`", db)).execute(&pool).await?;
    } else if let Some(db) = &conn.profile.database {
        sqlx::query(&format!("USE `{}`", db)).execute(&pool).await?;
    }

    // Execute and collect results
    let rows = sqlx::query(&sql).fetch_all(&pool).await
        .map_err(|e| anyhow::anyhow!("Query error: {}", e))?;

    if rows.is_empty() {
        println!("(no rows)");
        return Ok(());
    }

    let columns: Vec<String> = rows[0].columns().iter().map(|c| c.name().to_string()).collect();
    let data: Vec<Vec<String>> = rows.iter().map(|row| {
        (0..columns.len()).map(|i| {
            row.try_get::<String, _>(i)
                .or_else(|_| row.try_get::<i64, _>(i).map(|v| v.to_string()))
                .or_else(|_| row.try_get::<f64, _>(i).map(|v| v.to_string()))
                .or_else(|_| row.try_get::<bool, _>(i).map(|v| v.to_string()))
                .unwrap_or_else(|_| "NULL".to_string())
        }).collect()
    }).collect();

    ResultSet::new(columns, data).print(format)?;
    Ok(())
}
