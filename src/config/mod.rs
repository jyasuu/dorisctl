use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Defaults {
    pub profile: Option<String>,
    pub format: Option<String>,
    pub pager: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Profile {
    pub fe_host: String,
    #[serde(default = "default_mysql_port")]
    pub mysql_port: u16,
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    pub user: String,
    pub password: Option<String>,
    pub database: Option<String>,
}

fn default_mysql_port() -> u16 { 9030 }
fn default_http_port() -> u16 { 8030 }

impl Default for Profile {
    fn default() -> Self {
        Profile {
            fe_host: "localhost".into(),
            mysql_port: 9030,
            http_port: 8030,
            user: "root".into(),
            password: None,
            database: None,
        }
    }
}

impl Profile {
    pub fn mysql_url(&self) -> String {
        let pass = self.password.as_deref().unwrap_or("");
        let db = self.database.as_deref().unwrap_or("");
        format!("mysql://{}:{}@{}:{}/{}", self.user, pass, self.fe_host, self.mysql_port, db)
    }

    pub fn http_base(&self) -> String {
        format!("http://{}:{}", self.fe_host, self.http_port)
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,
}

impl Config {
    pub fn config_path() -> Result<PathBuf> {
        let base = dirs::config_dir().ok_or_else(|| anyhow!("Could not find config directory"))?;
        Ok(base.join("dorisctl").join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Config::default());
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Reading config at {}", path.display()))?;
        let cfg: Config = toml::from_str(&contents).with_context(|| "Parsing config file")?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;
        Ok(())
    }

    pub fn get_profile(&self, name: &str) -> Result<Profile> {
        if let Ok(host) = std::env::var("DORISCTL_HOST") {
            return Ok(Profile {
                fe_host: host,
                mysql_port: std::env::var("DORISCTL_MYSQL_PORT")
                    .ok().and_then(|p| p.parse().ok()).unwrap_or(9030),
                http_port: std::env::var("DORISCTL_HTTP_PORT")
                    .ok().and_then(|p| p.parse().ok()).unwrap_or(8030),
                user: std::env::var("DORISCTL_USER").unwrap_or_else(|_| "root".into()),
                password: std::env::var("DORISCTL_PASSWORD").ok(),
                database: std::env::var("DORISCTL_DATABASE").ok(),
            });
        }
        self.profiles.get(name).cloned().ok_or_else(|| anyhow!(
            "Profile '{}' not found. Run `dorisctl profile add {}` to create it.",
            name, name
        ))
    }
}
