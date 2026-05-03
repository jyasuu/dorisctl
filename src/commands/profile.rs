use anyhow::{anyhow, Result};
use clap::Subcommand;
use colored::Colorize;
use crate::config::{Config, Profile};

#[derive(Subcommand)]
pub enum ProfileCmd {
    /// Add or update a connection profile
    Add {
        /// Profile name
        name: String,
        /// FE host
        #[arg(long, default_value = "localhost")]
        fe_host: String,
        /// MySQL protocol port
        #[arg(long, default_value_t = 9030)]
        mysql_port: u16,
        /// HTTP REST port
        #[arg(long, default_value_t = 8030)]
        http_port: u16,
        /// Username
        #[arg(long, default_value = "root")]
        user: String,
        /// Password (prefer DORISCTL_PASSWORD env var)
        #[arg(long)]
        password: Option<String>,
        /// Default database
        #[arg(long)]
        database: Option<String>,
    },
    /// List all profiles
    List,
    /// Set the default profile
    Use {
        name: String,
    },
    /// Remove a profile
    Remove {
        name: String,
    },
}

pub async fn run(cmd: ProfileCmd, cfg: &Config) -> Result<()> {
    let mut cfg = cfg.clone();

    match cmd {
        ProfileCmd::Add { name, fe_host, mysql_port, http_port, user, password, database } => {
            let profile = Profile { fe_host, mysql_port, http_port, user, password, database };
            cfg.profiles.insert(name.clone(), profile);
            cfg.save()?;
            println!("{} Profile '{}' saved.", "✓".green(), name);
        }
        ProfileCmd::List => {
            if cfg.profiles.is_empty() {
                println!("No profiles configured. Use `dorisctl profile add <name>` to create one.");
                return Ok(());
            }
            let default = cfg.defaults.profile.as_deref().unwrap_or("");
            let mut table = comfy_table::Table::new();
            table.load_preset(comfy_table::presets::UTF8_FULL);
            table.set_header(["Name", "Host", "MySQL Port", "HTTP Port", "User", "Database", "Default"]);
            for (name, p) in &cfg.profiles {
                table.add_row([
                    name.as_str(),
                    &p.fe_host,
                    &p.mysql_port.to_string(),
                    &p.http_port.to_string(),
                    &p.user,
                    p.database.as_deref().unwrap_or("(none)"),
                    if name == default { "✓" } else { "" },
                ]);
            }
            println!("{table}");
        }
        ProfileCmd::Use { name } => {
            if !cfg.profiles.contains_key(&name) {
                return Err(anyhow!("Profile '{}' does not exist.", name));
            }
            cfg.defaults.profile = Some(name.clone());
            cfg.save()?;
            println!("{} Default profile set to '{}'.", "✓".green(), name);
        }
        ProfileCmd::Remove { name } => {
            if cfg.profiles.remove(&name).is_none() {
                return Err(anyhow!("Profile '{}' does not exist.", name));
            }
            cfg.save()?;
            println!("{} Profile '{}' removed.", "✓".green(), name);
        }
    }
    Ok(())
}

// Need Clone for Config
impl Clone for Config {
    fn clone(&self) -> Self {
        Config {
            defaults: self.defaults.clone(),
            profiles: self.profiles.clone(),
        }
    }
}
