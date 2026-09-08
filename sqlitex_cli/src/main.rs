use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "sqlitex")]
#[command(bin_name = "sqlitex")]
#[command(about = "CLI tool for sqlitex schema and migrations", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initializes sqlitex.toml and creates the migrations directory
    Init,

    /// Manage database migrations
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
}

#[derive(Subcommand)]
enum MigrateAction {
    /// Create a new migration file with the next sequential number
    Add {
        /// Name of the migration (e.g. create_users, add_media)
        name: String,
    },
}

#[derive(Deserialize)]
struct MinimalConfig {
    schema: Option<SchemaSection>,
}

#[derive(Deserialize)]
struct SchemaSection {
    path: String,
}

const DEFAULT_TOML_TEMPLATE: &str = r#"# sqlitex.toml

[schema]
path = "migrations/"

[database]
path = "app.db"

# [pragmas]
# All PRAGMA settings below are optional.
# By default, sqlitex already enforces foreign keys and a 5000ms busy timeout.
#
# foreign_keys = true       # Enforce relational constraints (REFERENCES other_table)
# busy_timeout = 5000       # Wait up to 5000ms if the database is locked before erroring
# journal_mode = "WAL"      # Enable Write-Ahead Log (5x-10x faster concurrent reads/writes)
# synchronous = "NORMAL"    # Reduces disk flushing overhead; safe to use with WAL mode
# cache_size = -64000       # Memory page cache (-64000 = ~64MB of RAM buffer)
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => run_init()?,
        Commands::Migrate { action } => match action {
            MigrateAction::Add { name } => run_migrate_add(&name)?,
        },
    }

    Ok(())
}

fn run_init() -> Result<(), Box<dyn std::error::Error>> {
    let toml_path = Path::new("sqlitex.toml");
    if toml_path.exists() {
        println!("⚠️  'sqlitex.toml' already exists. Skipping initialization.");
        return Ok(());
    }

    let mut file = File::create(toml_path)?;
    file.write_all(DEFAULT_TOML_TEMPLATE.as_bytes())?;
    println!(" Created 'sqlitex.toml'");

    let migrations_dir = Path::new("migrations");
    if !migrations_dir.exists() {
        fs::create_dir_all(migrations_dir)?;
        println!(" Created 'migrations/' directory");
    }

    let init_sql = migrations_dir.join("01_init.sql");
    if !init_sql.exists() {
        let mut sql_file = File::create(&init_sql)?;
        sql_file
            .write_all(b"-- Initial migration\n-- Write your CREATE TABLE statements here\n")?;
        println!(" Created 'migrations/01_init.sql'");
    }

    println!("\n Setup complete! Add your tables to 'migrations/01_init.sql'.");
    Ok(())
}

fn run_migrate_add(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let migrations_dir = resolve_migrations_dir()?;

    if !migrations_dir.exists() {
        fs::create_dir_all(&migrations_dir)?;
    }

    // Find all existing .sql files and determine the highest number
    let mut max_version = 0;
    if let Ok(entries) = fs::read_dir(&migrations_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("sql") {
                if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                    let num_str: String = filename
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    if let Ok(num) = num_str.parse::<i64>() {
                        max_version = max_version.max(num);
                    }
                }
            }
        }
    }

    let next_version = max_version + 1;
    let clean_name = name
        .trim()
        .replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    let filename = format!("{:02}_{}.sql", next_version, clean_name);
    let target_path = migrations_dir.join(&filename);

    let mut new_file = File::create(&target_path)?;
    writeln!(new_file, "-- Migration: {}", filename)?;

    println!(" Created migration: {}", target_path.display());
    Ok(())
}

fn resolve_migrations_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let toml_path = Path::new("sqlitex.toml");
    if toml_path.exists() {
        let content = fs::read_to_string(toml_path)?;
        if let Ok(cfg) = toml::from_str::<MinimalConfig>(&content) {
            if let Some(schema) = cfg.schema {
                return Ok(PathBuf::from(schema.path));
            }
        }
    }
    // Default fallback
    Ok(PathBuf::from("migrations"))
}
