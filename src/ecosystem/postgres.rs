
//! PostgreSQL schema export and comparison (feature = "postgres").
//!
//! No tokio-postgres dep needed — we translate Doris DDL to Postgres-compatible DDL
//! as a pure string transformation, and emit it to stdout or a file.
//! For running the DDL against a Postgres target, users pipe through `psql`.

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use crate::transport::Connection;

#[derive(Subcommand)]
pub enum PostgresCmd {
    /// Export a Doris table schema as PostgreSQL-compatible DDL
    ExportSchema {
        table: String,
        #[arg(short, long)]
        db: Option<String>,
        /// Output file (defaults to stdout)
        #[arg(short, long, value_name = "FILE")]
        output: Option<String>,
        /// Include CREATE TABLE IF NOT EXISTS guard
        #[arg(long)]
        if_not_exists: bool,
        /// Target PostgreSQL schema (default: public)
        #[arg(long, default_value = "public")]
        schema: String,
    },
    /// Export all tables in a database
    ExportAll {
        db: String,
        #[arg(short, long, value_name = "FILE")]
        output: Option<String>,
        #[arg(long, default_value = "public")]
        schema: String,
    },
    /// Compare Doris table schema with a Postgres DDL file
    Compare {
        table: String,
        #[arg(long, value_name = "FILE")]
        file: String,
        #[arg(short, long)]
        db: Option<String>,
    },
}

pub async fn run(cmd: PostgresCmd, conn: &Connection, _format: crate::output::Format) -> Result<()> {
    match cmd {
        PostgresCmd::ExportSchema { table, db, output, if_not_exists, schema } => {
            if let Some(d) = db.as_deref().or(conn.profile.database.as_deref()) {
                conn.use_db(d)?;
            }
            let ddl = export_table(conn, &table, &schema, if_not_exists)?;
            write_or_print(ddl, output.as_deref())?;
        }

        PostgresCmd::ExportAll { db, output, schema } => {
            conn.use_db(&db)?;
            let tables = conn.query("SHOW TABLES")?;
            let mut all = format!("-- Exported from Doris database '{}'\n\n", db);
            for row in &tables.rows {
                let tbl = &row[0];
                match export_table(conn, tbl, &schema, true) {
                    Ok(ddl) => { all.push_str(&ddl); all.push('\n'); }
                    Err(e)  => eprintln!("{} skipping '{}': {}", "warn:".yellow(), tbl, e),
                }
            }
            write_or_print(all, output.as_deref())?;
        }

        PostgresCmd::Compare { table, file, db } => {
            if let Some(d) = db.as_deref().or(conn.profile.database.as_deref()) {
                conn.use_db(d)?;
            }
            let generated = export_table(conn, &table, "public", true)?;
            let local = std::fs::read_to_string(&file)?;

            use similar::{ChangeTag, TextDiff};
            let diff = TextDiff::from_lines(generated.trim(), local.trim());
            let mut has_diff = false;
            println!("--- doris/{}", table);
            println!("+++ local/{}", file);
            for group in diff.grouped_ops(3) {
                for op in group {
                    for change in diff.iter_inline_changes(&op) {
                        let line: String = change.iter_strings_lossy().map(|(_, s)| s.to_string()).collect();
                        match change.tag() {
                            ChangeTag::Delete => { has_diff = true; print!("{}", format!("- {}", line).red()); }
                            ChangeTag::Insert => { has_diff = true; print!("{}", format!("+ {}", line).green()); }
                            ChangeTag::Equal  => print!("  {}", line),
                        }
                    }
                }
            }
            if !has_diff {
                println!("{}", "✓ No differences.".green());
            }
        }
    }
    Ok(())
}

fn export_table(conn: &Connection, table: &str, schema: &str, if_not_exists: bool) -> Result<String> {
    let r = conn.query(&format!("DESCRIBE `{}`", table))?;
    if r.rows.is_empty() {
        anyhow::bail!("Table '{}' not found or has no columns.", table);
    }

    let guard = if if_not_exists { "IF NOT EXISTS " } else { "" };
    let mut lines = vec![
        format!("-- {}.{}", schema, table),
        format!("CREATE TABLE {}{}\"{}\" (", guard, schema.to_string() + ".", table),
    ];

    let field_idx = col_idx(&r.columns, "Field").unwrap_or(0);
    let type_idx  = col_idx(&r.columns, "Type").unwrap_or(1);
    let null_idx  = col_idx(&r.columns, "Null");
    let key_idx   = col_idx(&r.columns, "Key");

    let mut pk_cols: Vec<String> = Vec::new();

    for (i, row) in r.rows.iter().enumerate() {
        let name = &row[field_idx];
        let pg_type = doris_to_pg_type(&row[type_idx]);
        let nullable = null_idx.map(|i| row[i].to_uppercase() == "YES").unwrap_or(true);
        if let Some(ki) = key_idx {
            if row[ki].to_uppercase() == "YES" || row[ki].to_uppercase() == "PRI" {
                pk_cols.push(format!("\"{}\"", name));
            }
        }
        let not_null = if !nullable { " NOT NULL" } else { "" };
        let comma = if i < r.rows.len() - 1 || !pk_cols.is_empty() { "," } else { "" };
        lines.push(format!("  \"{}\" {}{}{}", name, pg_type, not_null, comma));
    }
    if !pk_cols.is_empty() {
        lines.push(format!("  PRIMARY KEY ({})", pk_cols.join(", ")));
    }
    lines.push(");".to_string());
    Ok(lines.join("\n") + "\n")
}

fn doris_to_pg_type(doris_type: &str) -> &'static str {
    let t = doris_type.to_uppercase();
    let base = t.split('(').next().unwrap_or(&t).trim();
    match base {
        "TINYINT"    => "SMALLINT",
        "SMALLINT"   => "SMALLINT",
        "INT"        => "INTEGER",
        "BIGINT"     => "BIGINT",
        "LARGEINT"   => "NUMERIC(38,0)",
        "FLOAT"      => "REAL",
        "DOUBLE"     => "DOUBLE PRECISION",
        "DECIMAL"    => "NUMERIC",
        "BOOLEAN"    => "BOOLEAN",
        "CHAR"       => "CHAR",
        "VARCHAR"    => "VARCHAR",
        "STRING"     => "TEXT",
        "TEXT"       => "TEXT",
        "DATE"       => "DATE",
        "DATEV2"     => "DATE",
        "DATETIME"   => "TIMESTAMP",
        "DATETIMEV2" => "TIMESTAMP",
        "JSON"       => "JSONB",
        "ARRAY"      => "JSONB",
        "MAP"        => "JSONB",
        "STRUCT"     => "JSONB",
        "HLL"        => "BYTEA",
        "BITMAP"     => "BYTEA",
        _            => "TEXT",
    }
}

fn col_idx(columns: &[String], name: &str) -> Option<usize> {
    columns.iter().position(|c| c.to_lowercase() == name.to_lowercase())
}

fn write_or_print(content: String, path: Option<&str>) -> Result<()> {
    match path {
        Some(p) => {
            std::fs::write(p, &content)?;
            println!("{} Written to '{}'.", "✓".green(), p);
        }
        None => print!("{}", content),
    }
    Ok(())
}