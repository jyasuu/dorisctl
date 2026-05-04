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
    if let Some(db) = &args.database {
        conn.use_db(db)?;
    } else if let Some(db) = &conn.profile.database {
        conn.use_db(db)?;
    }

    match (args.sql, args.file) {
        (None, None) => run_repl(conn, format),
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
        "  Type SQL ending with {}  |  {} for help  |  {} or Ctrl-D to quit\n",
        ";".yellow(), "\\?".yellow(), "\\q".yellow()
    );

    // Track current catalog so prompt can show context
    let mut current_catalog: Option<String> = None;
    let mut current_db: Option<String> = None;
    let mut buf = String::new();

    loop {
        // Build prompt showing catalog.db context
        let ctx = match (&current_catalog, &current_db) {
            (Some(cat), Some(db)) => format!("[{}.{}]", cat, db),
            (Some(cat), None)     => format!("[{}]", cat),
            _                     => String::new(),
        };
        let prompt = if buf.trim().is_empty() {
            if ctx.is_empty() {
                format!("{} ", "doris>".cyan().bold())
            } else {
                format!("{}{} ", "doris".cyan().bold(), ctx.dimmed())
            }
        } else {
            format!("{} ", "     >".dimmed())
        };

        match rl.readline(&prompt) {
            Ok(line) => {
                let trimmed = line.trim();

                // ── Meta-commands ─────────────────────────────────────────────

                // Quit
                if trimmed == "\\q" || trimmed == "quit" || trimmed == "exit" {
                    if buf.trim().is_empty() {
                        println!("Bye!");
                        break;
                    }
                    println!("{}", "-- buffer cleared --".dimmed());
                    buf.clear();
                    continue;
                }

                // Help
                if trimmed == "\\?" || trimmed == "\\help" {
                    print_help();
                    continue;
                }

                // \l — list catalogs
                if trimmed == "\\l" {
                    exec_or_print_err(conn, "SHOW CATALOGS", format);
                    continue;
                }

                // \ldb — list databases in current catalog context
                if trimmed == "\\ldb" {
                    let sql = if let Some(cat) = &current_catalog {
                        format!("SHOW DATABASES FROM {}", cat)
                    } else {
                        "SHOW DATABASES".to_string()
                    };
                    exec_or_print_err(conn, &sql, format);
                    continue;
                }

                // \c catalog  — switch catalog (Doris: SWITCH `catalog`)
                // \c catalog.db — switch catalog then track db (no USE for external catalogs)
                if trimmed.starts_with("\\c ") {
                    let target = trimmed[3..].trim();
                    let parts: Vec<&str> = target.splitn(2, '.').collect();
                    let catalog = parts[0];
                    let db = parts.get(1).copied();

                    match exec_silent(conn, &format!("SWITCH `{}`", catalog)) {
                        Ok(_) => {
                            current_catalog = Some(catalog.to_string());
                            // Don't USE for external catalogs — just track db locally
                            current_db = db.map(|d| d.to_string());
                            match &current_db {
                                Some(d) => println!("Switched to \"{}.{}\".", catalog, d),
                                None    => println!("Switched to catalog \"{}\".", catalog),
                            }
                        }
                        Err(e) => eprintln!("{} {}", "error:".red(), e),
                    }
                    continue;
                }

                // \dt           — list tables; needs catalog.db context
                // \dt db        — list tables in catalog.db (uses current catalog)
                // \dt cat.db    — fully explicit
                if trimmed == "\\dt" || trimmed.starts_with("\\dt ") {
                    let arg = if trimmed.len() > 4 { trimmed[4..].trim() } else { "" };
                    let sql = if !arg.is_empty() {
                        // explicit arg: if it contains a dot, use as-is; else prepend current catalog
                        if arg.contains('.') {
                            format!("SHOW TABLES FROM {}", arg)
                        } else if let Some(cat) = &current_catalog {
                            format!("SHOW TABLES FROM {}.{}", cat, arg)
                        } else {
                            format!("SHOW TABLES FROM {}", arg)
                        }
                    } else {
                        // no arg: need catalog.db — use tracked values
                        match (&current_catalog, &current_db) {
                            (Some(cat), Some(db)) => format!("SHOW TABLES FROM {}.{}", cat, db),
                            (Some(cat), None) => {
                                eprintln!("{} no database selected — use {}  or  {}", 
                                    "hint:".yellow(), "\\dt <db>".yellow(), "\\c catalog.db".yellow());
                                format!("SHOW DATABASES") // fallback: show available dbs
                            }
                            _ => "SHOW TABLES".to_string(),
                        }
                    };
                    exec_or_print_err(conn, &sql, format);
                    continue;
                }

                // \d            — show tables in current context
                // \d name       — describe table (table / db.table / cat.db.table)
                if trimmed == "\\d" {
                    let sql = match (&current_catalog, &current_db) {
                        (Some(cat), Some(db)) => format!("SHOW TABLES FROM {}.{}", cat, db),
                        _ => "SHOW TABLES".to_string(),
                    };
                    exec_or_print_err(conn, &sql, format);
                    continue;
                }
                if trimmed.starts_with("\\d ") {
                    let arg = trimmed[3..].trim();
                    let part_count = arg.split('.').count();
                    // Doris external catalogs require fully-qualified catalog.db.table.
                    // Auto-prepend missing parts from tracked context.
                    let full = match part_count {
                        1 => match (&current_catalog, &current_db) {
                            (Some(cat), Some(db)) => format!("{}.{}.{}", cat, db, arg),
                            (Some(cat), None) => {
                                eprintln!("{} no database in context — use \\d cat.db.table", "hint:".yellow());
                                format!("{}.{}", cat, arg)
                            }
                            _ => arg.to_string(),
                        },
                        2 => match &current_catalog {
                            Some(cat) => format!("{}.{}", cat, arg),
                            None => arg.to_string(),
                        },
                        _ => arg.to_string(), // 3 parts: already fully qualified
                    };
                    let qualified = full.split('.')
                        .map(|p| format!("`{}`", p))
                        .collect::<Vec<_>>()
                        .join(".");
                    exec_or_print_err(conn, &format!("DESCRIBE {}", qualified), format);
                    continue;
                }

                // ── SQL buffer accumulation ───────────────────────────────────
                rl.add_history_entry(line.as_str())?;
                if !buf.is_empty() { buf.push('\n'); }
                buf.push_str(&line);

                if buf.trim_end().ends_with(';') {
                    let sql = buf.trim().trim_end_matches(';').trim().to_string();
                    buf.clear();
                    if sql.is_empty() { continue; }

                    // Track SWITCH / USE issued as raw SQL
                    let up = sql.trim_start().to_ascii_uppercase();
                    if up.starts_with("SWITCH ") {
                        let cat = sql[7..].trim().trim_matches('`').to_string();
                        current_catalog = Some(cat);
                        current_db = None;
                    } else if up.starts_with("USE ") {
                        let db = sql[4..].trim().trim_matches('`').to_string();
                        current_db = Some(db);
                    }

                    let start = std::time::Instant::now();
                    match exec_sql(conn, &sql, format) {
                        Ok(_) => println!("{}", format!("Time: {:.3}s", start.elapsed().as_secs_f64()).dimmed()),
                        Err(e) => eprintln!("{} {}", "error:".red(), e),
                    }
                }
            }

            Err(ReadlineError::Interrupted) => {
                if !buf.trim().is_empty() {
                    println!("{}", "-- cancelled --".dimmed());
                    buf.clear();
                } else {
                    println!("(Use \\q to quit)");
                }
            }
            Err(ReadlineError::Eof) => { println!("\nBye!"); break; }
            Err(e) => { eprintln!("{} {}", "readline error:".red(), e); break; }
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}

/// Run SQL, print error if it fails (for meta-commands where we don't want to propagate)
fn exec_or_print_err(conn: &Connection, sql: &str, format: Format) {
    if let Err(e) = exec_sql(conn, sql, format) {
        eprintln!("{} {}", "error:".red(), e);
    }
}

/// Run SQL silently (no output), return result (for \c switching)
fn exec_silent(conn: &Connection, sql: &str) -> Result<()> {
    let up = sql.trim_start().to_ascii_uppercase();
    let is_read = up.starts_with("SELECT") || up.starts_with("SHOW")
        || up.starts_with("DESCRIBE") || up.starts_with("DESC")
        || up.starts_with("EXPLAIN") || up.starts_with("WITH");
    if is_read {
        conn.query(sql)?;
    } else {
        conn.execute(sql)?;
    }
    Ok(())
}

fn print_help() {
    println!();
    println!("  {}       exit", "\\q / exit / quit".yellow());
    println!("  {}            list catalogs", "\\l".yellow());
    println!("  {}           list databases in current catalog", "\\ldb".yellow());
    println!("  {}  [db]       list tables (current db or given db)", "\\dt".yellow());
    println!("  {}  [cat.db]   list tables with explicit catalog.db", "\\dt".yellow());
    println!("  {}  <name>     describe  (table / db.table / cat.db.table)", "\\d".yellow());
    println!("  {}  <cat[.db]> switch catalog, optionally set db too", "\\c".yellow());
    println!("  {}          this help", "\\?".yellow());
    println!();
    println!("  End SQL with {} to execute.  Multi-line supported.", ";".yellow());
    println!("  Ctrl-C cancels buffer.  Up/Down for history.");
    println!();
}

fn exec_sql(conn: &Connection, sql: &str, format: Format) -> Result<()> {
    let up = sql.trim_start().to_ascii_uppercase();
    let is_read = up.starts_with("SELECT") || up.starts_with("SHOW")
        || up.starts_with("DESCRIBE") || up.starts_with("DESC")
        || up.starts_with("EXPLAIN") || up.starts_with("WITH");
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
