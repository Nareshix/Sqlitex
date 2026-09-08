use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize, Clone)]
pub struct SqlitexConfig {
    pub schema: Option<SchemaConfig>,
    pub database: Option<DatabaseConfig>,
    pub pragmas: Option<PragmaConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SchemaConfig {
    pub path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PragmaConfig {
    #[serde(default = "default_busy_timeout")]
    pub busy_timeout: u32,
    #[serde(default = "default_foreign_keys")]
    pub foreign_keys: bool,
    pub journal_mode: Option<String>,
    pub synchronous: Option<String>,
    pub cache_size: Option<i32>,
}

fn default_busy_timeout() -> u32 { 5000 }
fn default_foreign_keys() -> bool { true }

impl SqlitexConfig {
    pub fn load_from_manifest(manifest_dir: &str) -> (Option<Self>, Option<PathBuf>) {
        let toml_path = Path::new(manifest_dir).join("sqlitex.toml");
        if toml_path.exists()
            && let Ok(content) = std::fs::read_to_string(&toml_path)
                && let Ok(config) = toml::from_str::<SqlitexConfig>(&content) {
                    return (Some(config), Some(toml_path));
                }
        (None, None)
    }
}