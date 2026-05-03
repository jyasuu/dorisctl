
use anyhow::{anyhow, Result};
use clap::Subcommand;
use colored::Colorize;
use dialoguer::Confirm;
use crate::config::{Config, Defaults, Profile};

#[derive(Subcommand)]
pub enum ProfileCmd {
    /// Add or update a connection profile
    Add {
        name: String,
        #[arg(long, default_value = "localhost")]
        fe_host: String,
        #[arg(long, default_value_t = 9030)]
        mysql_port: u16,
        #[arg(long, default_value_t = 8030)]
        http_port: u16,
        #[arg(long, default_value = "root")]
        user: String,
        /// Password (prefer DORISCTL_PASSWORD env var instead)
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        database: Option<String>,
    },
    /// List all profiles
    List,
    /// Set the default profile
    Use { name: String },
    /// Remove a profile (prompts for confirmation)
    Remove {
        name: String,
        /// Skip confirmation prompt
        #[arg(long, short)]
        yes: bool,
    },
    /// Test connectivity of a profile
    Test { name: Option<String> },
}

pub async fn run(cmd: ProfileCmd, cfg: &Config) -> Result<()> {
    let mut cfg = cfg.clone();

    match cmd {
        ProfileCmd::Add { name, fe_host, mysql_port, http_port, user, password, database } => {
            let profile = Profile { fe_host, mysql_port, http_port, user, password, database };
            let verb = if cfg.profiles.contains_key(&name) { "Updated" } else { "Added" };
            cfg.profiles.insert(name.clone(), profile);
            cfg.save()?;
            println!("{} {} profile '{}'.", "✓".green(), verb, name);
            if cfg.defaults.profile.is_none() {
                cfg.defaults.profile = Some(name.clone());
                cfg.save()?;
                println!("  (set as default profile)");
            }
        }

        ProfileCmd::List => {
            if cfg.profiles.is_empty() {
                println!("No profiles configured. Run `dorisctl profile add <name>` to create one.");
                return Ok(());
            }
            let default = cfg.defaults.profile.as_deref().unwrap_or("");
            let mut table = comfy_table::Table::new();
            table.load_preset(comfy_table::presets::UTF8_FULL);
            table.set_header(["Name", "Host", "MySQL Port", "HTTP Port", "User", "Database", "Default"]);
            let mut names: Vec<&String> = cfg.profiles.keys().collect();
            names.sort();
            for name in names {
                let p = &cfg.profiles[name];
                table.add_row([
                    name.as_str(),
                    &p.fe_host,
                    &p.mysql_port.to_string(),
                    &p.http_port.to_string(),
                    &p.user,
                    p.database.as_deref().unwrap_or("—"),
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
            println!("{} Default profile → '{}'.", "✓".green(), name);
        }

        ProfileCmd::Remove { name, yes } => {
            if !cfg.profiles.contains_key(&name) {
                return Err(anyhow!("Profile '{}' does not exist.", name));
            }
            if !yes {
                let ok = Confirm::new()
                    .with_prompt(format!("Remove profile '{}'?", name))
                    .default(false)
                    .interact()?;
                if !ok { println!("Aborted."); return Ok(()); }
            }
            cfg.profiles.remove(&name);
            if cfg.defaults.profile.as_deref() == Some(&name) {
                cfg.defaults.profile = None;
            }
            cfg.save()?;
            println!("{} Profile '{}' removed.", "✓".green(), name);
        }

        ProfileCmd::Test { name } => {
            let profile_name = name.as_deref()
                .or(cfg.defaults.profile.as_deref())
                .unwrap_or("default");
            let profile = cfg.get_profile(profile_name)?;
            print!("Testing MySQL connection to {}:{} … ", profile.fe_host, profile.mysql_port);
            let conn = crate::transport::Connection::new(profile.clone());
            match conn.query("SELECT 1") {
                Ok(_) => println!("{}", "OK".green()),
                Err(e) => println!("{} {}", "FAILED:".red(), e),
            }
            print!("Testing HTTP connection to {}:{} … ", profile.fe_host, profile.http_port);
            match conn.http().get_json("/api/health") {
                Ok(_) => println!("{}", "OK".green()),
                Err(e) => println!("{} {}", "FAILED:".red(), e),
            }
        }
    }
    Ok(())
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Config {
            defaults: self.defaults.clone(),
            profiles: self.profiles.clone(),
        }
    }
}

impl Clone for Defaults {
    fn clone(&self) -> Self {
        Defaults {
            profile: self.profile.clone(),
            format: self.format.clone(),
            pager: self.pager,
        }
    }
}