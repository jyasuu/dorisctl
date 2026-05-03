use anyhow::Result;
use clap::Subcommand;
use indicatif::{ProgressBar, ProgressStyle};
use crate::transport::Connection;

#[derive(Subcommand)]
pub enum LoadCmd {
    /// Stream Load a file into a table
    Stream {
        /// Target table in db.table format
        #[arg(long)]
        table: String,
        /// File to load
        #[arg(long)]
        file: String,
        /// File format: csv, json, parquet
        #[arg(long, default_value = "csv")]
        format: String,
        /// Column separator (CSV)
        #[arg(long, default_value = ",")]
        column_separator: String,
        /// Load label (auto-generated if not set)
        #[arg(long)]
        label: Option<String>,
    },
    /// Check load job status
    Status {
        #[arg(long)]
        label: String,
    },
    /// Cancel a load job
    Cancel {
        #[arg(long)]
        label: String,
    },
}

pub async fn run(cmd: LoadCmd, conn: &Connection) -> Result<()> {
    match cmd {
        LoadCmd::Stream { table, file, format, column_separator, label } => {
            let parts: Vec<&str> = table.splitn(2, '.').collect();
            if parts.len() != 2 {
                anyhow::bail!("--table must be in db.table format");
            }
            let (db, tbl) = (parts[0], parts[1]);
            let label = label.unwrap_or_else(|| {
                format!("dorisctl_{}_{}", tbl, chrono_label())
            });

            let path = std::path::Path::new(&file);
            if !path.exists() {
                anyhow::bail!("File not found: {}", file);
            }
            let file_size = std::fs::metadata(path)?.len();
            let data = std::fs::read(path)?;

            let pb = ProgressBar::new(file_size);
            pb.set_style(ProgressStyle::default_bar()
                .template("{spinner} [{elapsed_precise}] [{bar:40}] {bytes}/{total_bytes} {msg}")?
                .progress_chars("=>-"));
            pb.set_message(format!("Uploading → {}", table));

            let http = conn.http();
            let url_path = format!("/api/{}/{}/_stream_load", db, tbl);

            let resp = http.put(&url_path)
                .header("label", &label)
                .header("format", &format)
                .header("column_separator", &column_separator)
                .header("Expect", "100-continue")
                .body(data)
                .send().await?;

            pb.finish_with_message("Upload complete");

            let status = resp.status();
            let body: serde_json::Value = resp.json().await?;

            if status.is_success() {
                let txn_status = body["Status"].as_str().unwrap_or("unknown");
                let loaded = body["NumberLoadedRows"].as_i64().unwrap_or(0);
                let filtered = body["NumberFilteredRows"].as_i64().unwrap_or(0);
                println!("Status:       {}", txn_status);
                println!("Label:        {}", label);
                println!("Loaded rows:  {}", loaded);
                println!("Filtered rows: {}", filtered);
                if txn_status != "Success" {
                    println!("Error URL:    {}", body["ErrorURL"].as_str().unwrap_or("N/A"));
                }
            } else {
                anyhow::bail!("Stream Load failed ({}): {}", status, body);
            }
        }

        LoadCmd::Status { label } => {
            let http = conn.http();
            let path = format!("/api/_load_error_log?label={}", label);
            let body = http.get_json(&path).await
                .unwrap_or(serde_json::json!({"message": "label not found"}));
            println!("{}", serde_json::to_string_pretty(&body)?);
        }

        LoadCmd::Cancel { label } => {
            // Doris does not have a direct cancel endpoint via REST for Stream Load
            // Broker Load jobs can be cancelled via MySQL
            println!("To cancel broker load jobs, use:");
            println!("  dorisctl query \"CANCEL LOAD WHERE LABEL = '{}'\"", label);
        }
    }
    Ok(())
}

fn chrono_label() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    ts.to_string()
}
