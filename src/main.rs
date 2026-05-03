mod config;
mod transport;
mod commands;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "dorisctl",
    about = "Developer-first CLI for Apache Doris",
    version,
    arg_required_else_help = true,
)]
struct Cli {
    /// Profile to use (overrides default)
    #[arg(short, long, global = true, env = "DORISCTL_PROFILE")]
    profile: Option<String>,

    /// Output format: table, json, csv
    #[arg(short, long, global = true, default_value = "table", env = "DORISCTL_FORMAT")]
    format: output::Format,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage connection profiles
    Profile {
        #[command(subcommand)]
        action: commands::profile::ProfileCmd,
    },
    /// Run a SQL query
    Query(commands::query::QueryArgs),
    /// Schema inspection and DDL operations
    Schema {
        #[command(subcommand)]
        action: commands::schema::SchemaCmd,
    },
    /// Data ingestion (Stream Load, Broker Load)
    Load {
        #[command(subcommand)]
        action: commands::load::LoadCmd,
    },
    /// Cluster administration
    Admin {
        #[command(subcommand)]
        action: commands::admin::AdminCmd,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let cfg = config::Config::load()?;
    let profile_name = cli.profile
        .as_deref()
        .or(cfg.defaults.profile.as_deref())
        .unwrap_or("default");

    match cli.command {
        Commands::Profile { action } => {
            commands::profile::run(action, &cfg).await?;
        }
        Commands::Query(args) => {
            let profile = cfg.get_profile(profile_name)?;
            let conn = transport::Connection::new(profile);
            commands::query::run(args, &conn, cli.format).await?;
        }
        Commands::Schema { action } => {
            let profile = cfg.get_profile(profile_name)?;
            let conn = transport::Connection::new(profile);
            commands::schema::run(action, &conn, cli.format).await?;
        }
        Commands::Load { action } => {
            let profile = cfg.get_profile(profile_name)?;
            let conn = transport::Connection::new(profile);
            commands::load::run(action, &conn).await?;
        }
        Commands::Admin { action } => {
            let profile = cfg.get_profile(profile_name)?;
            let conn = transport::Connection::new(profile);
            commands::admin::run(action, &conn, cli.format).await?;
        }
    }

    Ok(())
}
