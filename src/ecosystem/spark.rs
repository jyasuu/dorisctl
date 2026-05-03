
//! Spark integration (feature = "spark").
//!
//! Two modes:
//!   1. `emit-ddl` — translate a Doris table schema into Spark-compatible DDL or DataFrame schema JSON.
//!   2. `emit-schema-json` — output a Spark StructType JSON for programmatic use.

use anyhow::Result;
use clap::Subcommand;
use std::collections::HashMap;
use crate::transport::Connection;

#[derive(Subcommand)]
pub enum SparkCmd {
    /// Emit a Spark-compatible CREATE TABLE DDL for a Doris table
    EmitDdl {
        table: String,
        #[arg(short, long)]
        db: Option<String>,
        /// Output file (defaults to stdout)
        #[arg(short, long, value_name = "FILE")]
        output: Option<String>,
        /// Emit as Spark SQL (default) or DataFrame schema JSON
        #[arg(long)]
        json: bool,
    },
    /// Emit Spark StructType JSON for all tables in a database
    EmitAll {
        db: String,
        #[arg(short, long, value_name = "DIR")]
        output_dir: Option<String>,
    },
}

pub async fn run(cmd: SparkCmd, conn: &Connection) -> Result<()> {
    match cmd {
        SparkCmd::EmitDdl { table, db, output, json } => {
            if let Some(d) = db.as_deref().or(conn.profile.database.as_deref()) {
                conn.use_db(d)?;
            }
            let r = conn.query(&format!("DESCRIBE `{}`", table))?;
            if r.rows.is_empty() {
                anyhow::bail!("Table '{}' not found or has no columns.", table);
            }

            let content = if json {
                emit_struct_type_json(&table, &r.columns, &r.rows)
            } else {
                emit_spark_sql(&table, &r.columns, &r.rows)
            };

            match output {
                Some(path) => {
                    std::fs::write(&path, &content)?;
                    println!("Written to '{}'.", path);
                }
                None => print!("{}", content),
            }
        }

        SparkCmd::EmitAll { db, output_dir } => {
            conn.use_db(&db)?;
            let tables = conn.query("SHOW TABLES")?;
            let dir = output_dir.unwrap_or_else(|| format!("{}_spark_schemas", db));
            std::fs::create_dir_all(&dir)?;

            for row in &tables.rows {
                let tbl = &row[0];
                match conn.query(&format!("DESCRIBE `{}`", tbl)) {
                    Ok(r) if !r.rows.is_empty() => {
                        let content = emit_struct_type_json(tbl, &r.columns, &r.rows);
                        let path = format!("{}/{}.json", dir, tbl);
                        std::fs::write(&path, &content)?;
                        println!("  → {}", path);
                    }
                    _ => eprintln!("  skipping '{}' (no columns)", tbl),
                }
            }
            println!("Done. {} schema(s) written to '{}'.", tables.rows.len(), dir);
        }
    }
    Ok(())
}

// ── Type mapping ─────────────────────────────────────────────────────────────

fn doris_to_spark_type(doris_type: &str) -> &'static str {
    let t = doris_type.to_uppercase();
    let t = t.trim();
    // Strip precision/scale suffixes for matching
    let base = t.split('(').next().unwrap_or(t).trim();
    match base {
        "TINYINT"   => "ByteType",
        "SMALLINT"  => "ShortType",
        "INT"       => "IntegerType",
        "BIGINT"    => "LongType",
        "LARGEINT"  => "DecimalType(38,0)",
        "FLOAT"     => "FloatType",
        "DOUBLE"    => "DoubleType",
        "DECIMAL"   => "DecimalType",
        "BOOLEAN"   => "BooleanType",
        "CHAR"      => "StringType",
        "VARCHAR"   => "StringType",
        "STRING"    => "StringType",
        "TEXT"      => "StringType",
        "DATE"      => "DateType",
        "DATETIME"  => "TimestampType",
        "DATEV2"    => "DateType",
        "DATETIMEV2"=> "TimestampType",
        "ARRAY"     => "ArrayType",
        "MAP"       => "MapType",
        "STRUCT"    => "StructType",
        "JSON"      => "StringType",
        "JSONB"     => "BinaryType",
        "HLL"       => "BinaryType",
        "BITMAP"    => "BinaryType",
        _           => "StringType",
    }
}

fn emit_spark_sql(table: &str, columns: &[String], rows: &[Vec<String>]) -> String {
    // Columns from DESCRIBE: Field, Type, Null, Key, Default, Extra
    let field_idx = col_idx(columns, "Field").unwrap_or(0);
    let type_idx  = col_idx(columns, "Type").unwrap_or(1);
    let null_idx  = col_idx(columns, "Null");

    let mut lines = vec![format!("CREATE TABLE IF NOT EXISTS spark_{} (", table)];
    for (i, row) in rows.iter().enumerate() {
        let name = &row[field_idx];
        let dtype = doris_to_spark_type(&row[type_idx]);
        let nullable = null_idx.map(|i| row[i].to_uppercase() == "YES").unwrap_or(true);
        let comma = if i < rows.len() - 1 { "," } else { "" };
        lines.push(format!("  `{}` {}{}{}", name, dtype, if !nullable { " NOT NULL" } else { "" }, comma));
    }
    lines.push(") USING DELTA;".to_string());
    lines.join("\n") + "\n"
}

fn emit_struct_type_json(table: &str, columns: &[String], rows: &[Vec<String>]) -> String {
    let field_idx = col_idx(columns, "Field").unwrap_or(0);
    let type_idx  = col_idx(columns, "Type").unwrap_or(1);
    let null_idx  = col_idx(columns, "Null");

    let fields: Vec<serde_json::Value> = rows.iter().map(|row| {
        let name = &row[field_idx];
        let spark_type = doris_to_spark_type(&row[type_idx]);
        let nullable = null_idx.map(|i| row[i].to_uppercase() == "YES").unwrap_or(true);
        serde_json::json!({
            "name": name,
            "type": spark_type,
            "nullable": nullable,
            "metadata": {}
        })
    }).collect();

    let schema = serde_json::json!({
        "type": "struct",
        "table": table,
        "fields": fields
    });

    serde_json::to_string_pretty(&schema).unwrap() + "\n"
}

fn col_idx(columns: &[String], name: &str) -> Option<usize> {
    columns.iter().position(|c| c.to_lowercase() == name.to_lowercase())
}