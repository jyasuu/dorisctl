use anyhow::Result;
use clap::ValueEnum;
use comfy_table::{Table, presets::UTF8_FULL};
use serde_json::Value;

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum Format {
    #[default]
    Table,
    Json,
    Csv,
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Format::Table => write!(f, "table"),
            Format::Json => write!(f, "json"),
            Format::Csv => write!(f, "csv"),
        }
    }
}

/// A simple result set with column names and rows
pub struct ResultSet {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl ResultSet {
    pub fn new(columns: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        Self { columns, rows }
    }

    pub fn print(&self, format: Format) -> Result<()> {
        match format {
            Format::Table => self.print_table(),
            Format::Json => self.print_json(),
            Format::Csv => self.print_csv(),
        }
    }

    fn print_table(&self) -> Result<()> {
        if self.rows.is_empty() {
            println!("(no rows)");
            return Ok(());
        }
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        table.set_header(self.columns.clone());
        for row in &self.rows {
            table.add_row(row);
        }
        println!("{table}");
        println!("{} row(s)", self.rows.len());
        Ok(())
    }

    fn print_json(&self) -> Result<()> {
        let records: Vec<serde_json::Map<String, Value>> = self.rows.iter().map(|row| {
            let mut map = serde_json::Map::new();
            for (col, val) in self.columns.iter().zip(row.iter()) {
                map.insert(col.clone(), Value::String(val.clone()));
            }
            map
        }).collect();
        println!("{}", serde_json::to_string_pretty(&records)?);
        Ok(())
    }

    fn print_csv(&self) -> Result<()> {
        let mut wtr = csv::Writer::from_writer(std::io::stdout());
        wtr.write_record(&self.columns)?;
        for row in &self.rows {
            wtr.write_record(row)?;
        }
        wtr.flush()?;
        Ok(())
    }
}
