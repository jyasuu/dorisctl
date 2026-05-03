
use anyhow::{bail, Result};
use clap::Subcommand;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use crate::transport::Connection;

#[derive(Subcommand)]
pub enum LoadCmd {
    /// Stream Load a local file into a Doris table
    Stream {
        /// Target table in db.table format
        #[arg(long)]
        table: String,
        /// Local file to load
        #[arg(long, value_name = "FILE")]
        file: String,
        /// File format: csv | json | parquet
        #[arg(long, default_value = "csv")]
        format: String,
        /// Column separator for CSV (default: comma)
        #[arg(long, default_value = ",")]
        column_separator: String,
        /// Line delimiter for CSV (default: \\n)
        #[arg(long, default_value = "\n")]
        line_delimiter: String,
        /// Custom load label (auto-generated if omitted)
        #[arg(long)]
        label: Option<String>,
        /// Columns mapping expression
        #[arg(long)]
        columns: Option<String>,
        /// WHERE predicate to filter rows
        #[arg(long)]
        where_expr: Option<String>,
        /// Maximum error rows allowed before aborting
        #[arg(long, default_value_t = 0)]
        max_filter_ratio_pct: u8,
        /// Preview: print what would be sent, but don't upload
        #[arg(long)]
        dry_run: bool,
    },
    /// Broker Load a remote dataset
    Broker {
        /// Job name
        #[arg(long)]
        job_name: String,
        /// Data source path
        #[arg(long)]
        data_source: String,
        /// Broker name
        #[arg(long)]
        broker: String,
        /// Target table in db.table format
        #[arg(long)]
        table: String,
        /// Preview without executing
        #[arg(long)]
        dry_run: bool,
    },
    /// Check the status of a load job by label
    Status {
        #[arg(long)]
        label: String,
        /// Database that owns the job
        #[arg(long)]
        db: Option<String>,
    },
    /// Cancel a load job by label
    Cancel {
        #[arg(long)]
        label: String,
        /// Database that owns the job
        #[arg(long)]
        db: Option<String>,
    },
}

pub async fn run(cmd: LoadCmd, conn: &Connection) -> Result<()> {
    match cmd {
        LoadCmd::Stream {
            table, file, format, column_separator, line_delimiter,
            label, columns, where_expr, max_filter_ratio_pct, dry_run,
        } => {
            let parts: Vec<&str> = table.splitn(2, '.').collect();
            if parts.len() != 2 {
                bail!("--table must be in db.table format (e.g. mydb.orders)");
            }
            let (db, tbl) = (parts[0], parts[1]);

            let path = std::path::Path::new(&file);
            if !path.exists() {
                bail!("File not found: {}", file);
            }
            let file_size = std::fs::metadata(path)?.len();
            let label = label.unwrap_or_else(|| {
                format!("dorisctl_{}_{}", tbl, unix_ts())
            });

            if dry_run {
                println!("{}", "-- Stream Load dry-run (not uploaded) --".yellow());
                println!("  endpoint : http://{}:{}/api/{}/{}/_stream_load", conn.profile.fe_host, conn.profile.http_port, db, tbl);
                println!("  label    : {}", label);
                println!("  file     : {} ({} bytes)", file, file_size);
                println!("  format   : {}", format);
                return Ok(());
            }

            // Read file
            let data = std::fs::read(path)?;

            // Progress bar
            let pb = ProgressBar::new(file_size);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                    .progress_chars("=>-"),
            );
            pb.set_message(format!("Uploading → {}", table));

            let url_path = format!("/api/{}/{}/_stream_load", db, tbl);
            let mut headers: Vec<(&str, &str)> = vec![
                ("label", label.as_str()),
                ("format", format.as_str()),
                ("column_separator", column_separator.as_str()),
                ("line_delimiter", line_delimiter.as_str()),
                ("Expect", "100-continue"),
            ];
            let col_str;
            if let Some(ref c) = columns {
                col_str = c.clone();
                headers.push(("columns", col_str.as_str()));
            }
            let where_str;
            if let Some(ref w) = where_expr {
                where_str = w.clone();
                headers.push(("where", where_str.as_str()));
            }
            let ratio_str = format!("{:.2}", max_filter_ratio_pct as f64 / 100.0);
            headers.push(("max_filter_ratio", ratio_str.as_str()));

            let body = conn.http().put_bytes(&url_path, &headers, data)?;
            pb.finish_and_clear();

            print_load_result(&body, &label);
        }

        LoadCmd::Broker { job_name, data_source, broker, table, dry_run } => {
            let parts: Vec<&str> = table.splitn(2, '.').collect();
            if parts.len() != 2 {
                bail!("--table must be in db.table format");
            }
            let (db, tbl) = (parts[0], parts[1]);
            let sql = format!(
                "LOAD LABEL `{}`.`{}`\n(\n  DATA INFILE('{}')\n  INTO TABLE `{}`\n)\nWITH BROKER '{}';",
                db, job_name, data_source, tbl, broker
            );

            if dry_run {
                println!("{}", "-- Broker Load dry-run (not executed) --".yellow());
                println!("{}", sql);
                return Ok(());
            }

            conn.execute(&sql)?;
            println!("{} Broker Load job '{}' submitted.", "✓".green(), job_name);
            println!("Check status with: dorisctl load status --label {} --db {}", job_name, db);
        }

        LoadCmd::Status { label, db } => {
            if let Some(db) = &db { conn.use_db(db)?; }
            let sql = format!("SHOW LOAD WHERE LABEL = '{}'", label);
            let r = conn.query(&sql)?;
            if r.rows.is_empty() {
                println!("No load job found with label '{}'.", label);
            } else {
                crate::output::ResultSet::new(r.columns, r.rows)
                    .print(crate::output::Format::Table)?;
            }
        }

        LoadCmd::Cancel { label, db } => {
            if let Some(db) = &db { conn.use_db(db)?; }
            conn.execute(&format!("CANCEL LOAD WHERE LABEL = '{}'", label))?;
            println!("{} Load job '{}' cancelled.", "✓".green(), label);
        }
    }
    Ok(())
}

fn print_load_result(body: &serde_json::Value, label: &str) {
    let status = body["Status"].as_str().unwrap_or("Unknown");
    let loaded  = body["NumberLoadedRows"].as_i64().unwrap_or(0);
    let filtered = body["NumberFilteredRows"].as_i64().unwrap_or(0);
    let msg     = body["Message"].as_str().unwrap_or("");

    if status == "Success" {
        println!("{} Stream Load succeeded.", "✓".green());
    } else {
        println!("{} Stream Load status: {}", "✗".red(), status);
    }
    println!("  label    : {}", label);
    println!("  loaded   : {}", loaded);
    println!("  filtered : {}", filtered);
    if !msg.is_empty() { println!("  message  : {}", msg); }
    if let Some(url) = body["ErrorURL"].as_str() {
        if !url.is_empty() { println!("  error URL: {}", url); }
    }
}

fn unix_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}