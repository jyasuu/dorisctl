
use anyhow::{bail, Result};
use clap::Args;
use colored::Colorize;
use rustyline::error::ReadlineError;
use rustyline::history::FileHistory;
use rustyline::{CompletionType, Config, EditMode, Editor};
use crate::output::{Format, ResultSet};
use crate::transport::Connection;

#[derive(Args)]
pub struct QueryArgs {
    /// SQL query string (omit to enter interactive mode)
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
    // Switch database if requested
    if let Some(db) = &args.database {
        conn.use_db(db)?;
    } else if let Some(db) = &conn.profile.database {
        conn.use_db(db)?;
    }

    match (args.sql, args.file) {
        // Interactive mode — no SQL provided
        (None, None) => run_repl(conn, format),
        // One-shot mode
        (sql, file) => {
            let sql = resolve_sql(sql, file)?;
            if args.dry_run {
                println!("-- dry-run (not executed) --");
                println!("{}", sql);
                return Ok(());
            }
            exec_sql(conn, &sql, format)
        }
    }
}

/// Interactive REPL — accumulates multi-line input until a `;` is found.
fn run_repl(conn: &Connection, format: Format) -> Result<()> {
    let history_path = dirs::data_local_dir()
        .map(|d| d.join("dorisctl").join("history"))
        .unwrap_or_else(|| std::path::PathBuf::from(".dorisctl_history"));

    if let Some(parent) = history_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let config = Config::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .build();

    let mut rl: Editor<(), FileHistory> = Editor::with_config(config)?;
    let _ = rl.load_history(&history_path);

    println!(
        "{} {}  {}",
        "dorisctl".cyan().bold(),
        env!("CARGO_PKG_VERSION"),
        format!("connected to {}", conn.profile.fe_host).dimmed()
    );
    println!(
        "  Type SQL and end with {} to execute.  {} to quit.\n",
        ";".yellow(),
        "\\q or Ctrl-D".yellow()
    );

    let mut buf = String::new();

    loop {
        let prompt = if buf.trim().is_empty() {
            format!("{} ", "doris>".cyan().bold())
        } else {
            format!("{} ", "     >".dimmed())
        };

        match rl.readline(&prompt) {
            Ok(line) => {
                let trimmed = line.trim();

                // Exit commands
                if buf.trim().is_empty() {
                    if trimmed == "\\q" || trimmed == "quit" || trimmed == "exit" {
                        println!("Bye!");
                        break;
                    }
                    // Meta-commands
                    if trimmed == "\\?" || trimmed == "\\help" {
                        print_help();
                        continue;
                    }
                    if trimmed.starts_with("\\c ") {
                        let db = trimmed[3..].trim();
                        match conn.use_db(db) {
                            Ok(_) => println!("You are now connected to database \"{}\".", db),
                            Err(e) => eprintln!("{} {}", "error:".red(), e),
                        }
                        continue;
                    }
                    if trimmed == "\\l" {
                        let _ = exec_sql(conn, "SHOW DATABASES", format);
                        continue;
                    }
                    if trimmed == "\\dt" || trimmed == "\\d" {
                        let _ = exec_sql(conn, "SHOW TABLES", format);
                        continue;
                    }
                    if trimmed.starts_with("\\d ") {
                        let table = trimmed[3..].trim();
                        let _ = exec_sql(conn, &format!("DESCRIBE `{}`", table), format);
                        continue;
                    }
                }

                rl.add_history_entry(line.as_str())?;

                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&line);

                // Execute when we see a semicolon at end of trimmed buffer
                if buf.trim_end().ends_with(';') {
                    let sql = buf.trim().trim_end_matches(';').trim().to_string();
                    buf.clear();

                    if sql.is_empty() {
                        continue;
                    }

                    let start = std::time::Instant::now();
                    match exec_sql(conn, &sql, format) {
                        Ok(_) => {
                            println!("{}", format!("Time: {:.3}s", start.elapsed().as_secs_f64()).dimmed());
                        }
                        Err(e) => eprintln!("{} {}", "error:".red(), e),
                    }
                }
            }

            Err(ReadlineError::Interrupted) => {
                // Ctrl-C clears current buffer like psql
                if !buf.trim().is_empty() {
                    println!("{}", "-- cancelled --".dimmed());
                    buf.clear();
                } else {
                    println!("(Use \\q to quit)");
                }
            }

            Err(ReadlineError::Eof) => {
                println!("\nBye!");
                break;
            }

            Err(e) => {
                eprintln!("{} {}", "readline error:".red(), e);
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}

fn print_help() {
    println!();
    println!("  {}  exit the REPL", "\\q / exit / quit".yellow());
    println!("  {}        list databases", "\\l".yellow());
    println!("  {}        list tables in current database", "\\dt".yellow());
    println!("  {}  <table>  describe a table", "\\d".yellow());
    println!("  {}  <db>     switch database", "\\c".yellow());
    println!("  {}       show this help", "\\?".yellow());
    println!("  End any SQL with {} to execute", ";".yellow());
    println!("  Multi-line input is supported — press Enter to continue");
    println!("  Arrow keys / history: Up/Down navigate previous queries");
    println!();
}

fn exec_sql(conn: &Connection, sql: &str, format: Format) -> Result<()> {
    let trimmed = sql.trim_start().to_ascii_uppercase();
    let is_read = trimmed.starts_with("SELECT")
        || trimmed.starts_with("SHOW")
        || trimmed.starts_with("DESCRIBE")
        || trimmed.starts_with("DESC")
        || trimmed.starts_with("EXPLAIN")
        || trimmed.starts_with("WITH");

    if is_read {
        let result = conn.query(sql)?;
        ResultSet::new(result.columns, result.rows).print(format)?;
    } else {
        let affected = conn.execute(sql)?;
        println!("OK — {} row(s) affected.", affected);
    }
    Ok(())
}

pub fn resolve_sql(sql: Option<String>, file: Option<String>) -> Result<String> {
    match (sql, file) {
        (Some(s), None) => Ok(s),
        (None, Some(f)) => Ok(std::fs::read_to_string(&f)
            .map_err(|e| anyhow::anyhow!("Cannot read '{}': {}", f, e))?),
        (Some(_), Some(_)) => bail!("Provide either a SQL string or --file, not both."),
        (None, None) => bail!("Provide a SQL string, --file <path>, or omit both for interactive mode."),
    }
}
