
use anyhow::Result;
use clap::Subcommand;
use similar::{ChangeTag, TextDiff};
use colored::Colorize;
use crate::output::{Format, ResultSet};
use crate::transport::Connection;

#[derive(Subcommand)]
pub enum SchemaCmd {
    /// List all databases
    ListDbs,
    /// List tables in a database
    ListTables {
        #[arg(short, long)]
        db: Option<String>,
    },
    /// Show column definitions for a table
    Describe {
        table: String,
        #[arg(short, long)]
        db: Option<String>,
    },
    /// Apply DDL statements from a file
    Apply {
        #[arg(long, value_name = "FILE")]
        file: String,
        /// Print statements but do not execute
        #[arg(long)]
        dry_run: bool,
    },
    /// Diff a local DDL file against the live CREATE TABLE
    Diff {
        table: String,
        #[arg(long, value_name = "FILE")]
        file: String,
        #[arg(short, long)]
        db: Option<String>,
    },
}

pub async fn run(cmd: SchemaCmd, conn: &Connection, format: Format) -> Result<()> {
    match cmd {
        SchemaCmd::ListDbs => {
            let r = conn.query("SHOW DATABASES")?;
            ResultSet::new(r.columns, r.rows).print(format)?;
        }

        SchemaCmd::ListTables { db } => {
            set_db(conn, db.as_deref())?;
            let r = conn.query("SHOW TABLES")?;
            ResultSet::new(r.columns, r.rows).print(format)?;
        }

        SchemaCmd::Describe { table, db } => {
            set_db(conn, db.as_deref())?;
            let r = conn.query(&format!("DESCRIBE `{}`", table))?;
            if r.rows.is_empty() {
                println!("Table '{}' not found or has no columns.", table);
            } else {
                ResultSet::new(r.columns, r.rows).print(format)?;
            }
        }

        SchemaCmd::Apply { file, dry_run } => {
            let sql = std::fs::read_to_string(&file)
                .map_err(|e| anyhow::anyhow!("Cannot read '{}': {}", file, e))?;
            let stmts: Vec<&str> = sql
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();

            println!("{} statement(s) in '{}'", stmts.len(), file);

            if dry_run {
                for (i, stmt) in stmts.iter().enumerate() {
                    let preview = &stmt[..stmt.len().min(120)];
                    println!("[{}] {}{}", i + 1, preview, if stmt.len() > 120 { " …" } else { "" });
                }
                println!("{}", "(dry-run — nothing executed)".yellow());
            } else {
                for (i, stmt) in stmts.iter().enumerate() {
                    conn.execute(stmt)
                        .map_err(|e| anyhow::anyhow!("Statement {} failed: {}", i + 1, e))?;
                    println!("[{}] {}", i + 1, "OK".green());
                }
            }
        }

        SchemaCmd::Diff { table, file, db } => {
            set_db(conn, db.as_deref())?;

            let local = std::fs::read_to_string(&file)
                .map_err(|e| anyhow::anyhow!("Cannot read '{}': {}", file, e))?;

            let r = conn.query(&format!("SHOW CREATE TABLE `{}`", table))
                .map_err(|_| anyhow::anyhow!("Table '{}' not found.", table))?;

            let live = r.rows.first()
                .and_then(|row| row.get(1))
                .cloned()
                .unwrap_or_default();

            if local.trim() == live.trim() {
                println!("{}", "✓ No differences between local file and live schema.".green());
                return Ok(());
            }

            // Unified diff
            let diff = TextDiff::from_lines(live.trim(), local.trim());
            println!("--- live/{}", table);
            println!("+++ local/{}", file);
            for group in diff.grouped_ops(3) {
                for op in &group {
                    for change in diff.iter_changes(op) {
                        let line = change.to_string_lossy().to_string();
                        match change.tag() {
                            ChangeTag::Delete => print!("{}", format!("- {}", line).red()),
                            ChangeTag::Insert => print!("{}", format!("+ {}", line).green()),
                            ChangeTag::Equal  => print!("  {}", line),
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn set_db(conn: &Connection, db: Option<&str>) -> Result<()> {
    if let Some(d) = db.or(conn.profile.database.as_deref()) {
        conn.use_db(d)?;
    }
    Ok(())
}