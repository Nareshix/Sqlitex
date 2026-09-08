use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize, Clone)]
pub struct SqlitexConfig {
    pub schema: Option<SchemaConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SchemaConfig {
    pub path: String,
}

impl SqlitexConfig {
    pub fn load_from_manifest(manifest_dir: &str) -> (Option<Self>, Option<PathBuf>) {
        let toml_path = Path::new(manifest_dir).join("sqlitex.toml");
        if toml_path.exists()
            && let Ok(content) = std::fs::read_to_string(&toml_path)
            && let Ok(config) = toml::from_str::<SqlitexConfig>(&content)
        {
            return (Some(config), Some(toml_path));
        }
        (None, None)
    }
}