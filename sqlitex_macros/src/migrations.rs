use std::collections::HashMap;

use quote::quote;
use sqlitex_type_inference::validate_create_table_types;
use sqlitex_type_inference::{table::create_tables, validate_cast_types};

use crate::utils::fnv1a_hash;

pub(crate) struct MigrationsOutput {
    pub schema_init_method: proc_macro2::TokenStream,
    pub watcher_tokens: proc_macro2::TokenStream,
    pub all_tables: HashMap<String, Vec<sqlitex_type_inference::table::ColumnInfo>>,
}

pub(crate) fn process_migrations_dir(
    path: &syn::LitStr,
    db_path: &str,
) -> syn::Result<MigrationsOutput> {
    let mut all_tables = HashMap::new();
    let watcher_tokens;
    let schema_init_method;

    let mut files: Vec<_> = std::fs::read_dir(db_path)
        .map_err(|e| {
            syn::Error::new(
                path.span(),
                format!("Failed to read directory {}: {}", db_path, e),
            )
        })?
        .filter_map(|res| res.ok())
        .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("sql"))
        .collect();

    // EMPTY FOLDER CHECK
    if files.is_empty() {
        return Err(syn::Error::new(
            path.span(),
            format!(
                "No .sql files detected in the migrations directory: '{}'",
                db_path
            ),
        ));
    }

    files.sort_by_key(|entry| {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let num_str: String = file_name
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        num_str.parse::<i32>().unwrap_or(i32::MAX)
    });

    // We will store the file_name and content to pass to sqlite3_exec
    let mut script_batches = Vec::new();
    let mut migration_embeds = Vec::new();
    let mut watcher_includes = Vec::new();
    let mut seen_versions = std::collections::HashSet::new();

    for entry in files {
        let file_path = entry.path();
        let file_path_str = file_path.to_str().unwrap().to_string();
        let file_name = entry.file_name().to_string_lossy().to_string();

        let num_str: String = file_name
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let version: i64 = num_str.parse().map_err(|_| {
            syn::Error::new(
                path.span(),
                format!("Migration filename must start with a number: {}", file_name),
            )
        })?;

        if !seen_versions.insert(version) {
            return Err(syn::Error::new(
                path.span(),
                format!(
                    "Duplicate migration version number detected: {}. Migration numbers must be unique.",
                    version
                ),
            ));
        }

        let content = std::fs::read_to_string(&file_path).map_err(|e| {
            syn::Error::new(path.span(), format!("Failed to read {}: {}", file_name, e))
        })?;

        // Per-file type inference & syntax checks
        sqlitex_type_inference::validate_sql_file_syntax(&content).map_err(|msg| {
            syn::Error::new(
                path.span(),
                format!("In migration file '{}': {}", file_name, msg),
            )
        })?;
        validate_cast_types(&content).map_err(|msg| {
            syn::Error::new(
                path.span(),
                format!("In migration file '{}': {}", file_name, msg),
            )
        })?;
        validate_create_table_types(&content).map_err(|msg| {
            syn::Error::new(
                path.span(),
                format!("In migration file '{}': {}", file_name, msg),
            )
        })?;

        // Push to our batches for SQLite execution
        script_batches.push((file_name.clone(), content.clone()));

        watcher_includes.push(quote! {
            const _: &[u8] = include_bytes!(#file_path_str);
        });

        let checksum = fnv1a_hash(&content);
        let checksum_tokens: proc_macro2::TokenStream = checksum.to_string().parse().unwrap();
        migration_embeds.push(quote! {
            (#version, #file_name, #checksum_tokens, include_str!(#file_path_str))
        });
    }

    watcher_tokens = quote! { #(#watcher_includes)* };

    // Boot up a real SQLite instance in the compiler and test the scripts file-by-file
    let schemas = sqlitex_core::utility::utils::get_db_schema_from_statements(&script_batches)
        .map_err(|err| {
            // This will now output: "In file '02_bad.sql': UNIQUE constraint failed: items.id"
            syn::Error::new(path.span(), err)
        })?;

    for schema in schemas {
        validate_create_table_types(&schema).map_err(|msg| {
            syn::Error::new(path.span(), format!("In migrations schema: {}", msg))
        })?;
        create_tables(&schema, &mut all_tables);
    }

    let doc_msg = "Applies all pending migrations from the directory in numerical order. Uses an internal `_sqlitex_migrations` tracking table to ensure each migration is applied only once and atomically.";
    schema_init_method = quote! {
                    #[doc = #doc_msg]
                    pub fn migrate(&mut self) -> Result<(), sqlitex::errors::Error> {
                        let migrations = vec![
                            #(#migration_embeds),*
                        ];

                        // Wrap the entire migration process in a single transaction.
                        // This prevents race conditions if multiple instances start simultaneously.
                        self.transaction(|tx| {
                            tx.__db.execute_batch(
                                "CREATE TABLE IF NOT EXISTS _sqlitex_migrations (
                                version INTEGER PRIMARY KEY,
                                name TEXT NOT NULL,
                                checksum INTEGER NOT NULL
                            );"
                            )?;

                            let mut applied_versions = std::collections::HashSet::new();
                            if let Ok(rows) = tx.__db.query("SELECT version, name, checksum FROM _sqlitex_migrations ORDER BY version ASC") {
                                for row in rows.all()? {
                                    let db_version = row[0].as_i64();
                                    let db_name = row[1].as_string();
                                    let db_checksum = row[2].as_i64();

                                    applied_versions.insert(db_version);

                                    if let Some((_, disk_name, disk_checksum, _)) = migrations.iter().find(|m| m.0 == db_version) {
                                        if db_name != *disk_name {
                                            return Err(sqlitex::errors::Error::Migration(
                                                sqlitex::errors::MigrationError::NameMismatch {
                                                    version: db_version,
                                                    expected_name: db_name,
                                                    actual_name: disk_name.to_string(),
                                                }
                                            ));
                                        }
                                        if db_checksum != *disk_checksum {
                                            return Err(sqlitex::errors::Error::Migration(
                                                sqlitex::errors::MigrationError::ChecksumMismatch {
                                                    version: db_version,
                                                    name: db_name,
                                                    expected_checksum: db_checksum,
                                                    actual_checksum: *disk_checksum,
                                                }
                                            ));
                                        }
                                    } else {
                                        return Err(sqlitex::errors::Error::Migration(
                                            sqlitex::errors::MigrationError::MissingFile {
                                                version: db_version,
                                                name: db_name,
                                            }
                                        ));
                                    }
                                }
                            }

                            for (version, name, checksum, sql) in migrations {
                                if !applied_versions.contains(&version) {
                                    // If it hasn't been applied, run it!
                                    tx.__db.execute_batch(sql)?;

                                    let mut stmt = std::ptr::null_mut();
                                    unsafe {
                                        sqlitex::utility::utils::prepare_stmt(
                                            tx.__db.db,
                                            &mut stmt,
                                            "INSERT INTO _sqlitex_migrations (version, name, checksum) VALUES (?, ?, ?)"
                                        ).map_err(|e| sqlitex::errors::Error::from(sqlitex::errors::SqlWriteBindingError::Prepare(e)))?;
                                    }

                                    let preparred_statement = sqlitex::internal_sqlite::preparred_statement::PreparredStmt {
                                        stmt,
                                        conn: tx.__db.db,
                                    };

                                    preparred_statement.bind_parameter(1, version).map_err(|e| sqlitex::errors::Error::from(sqlitex::errors::SqlWriteBindingError::Bind(e)))?;
                                    preparred_statement.bind_parameter(2, name).map_err(|e| sqlitex::errors::Error::from(sqlitex::errors::SqlWriteBindingError::Bind(e)))?;
                                    preparred_statement.bind_parameter(3, checksum).map_err(|e| sqlitex::errors::Error::from(sqlitex::errors::SqlWriteBindingError::Bind(e)))?;

    // Wrap in ManuallyDrop so rust doesn't call `PreparredStmt::drop`.
    // If it did, it would call sqlite3_reset on a freed pointer, leading to free after use error
    let mut preparred_statement_mut = std::mem::ManuallyDrop::new(preparred_statement);

    let step_result = preparred_statement_mut.step().map_err(|e| sqlitex::errors::Error::from(sqlitex::errors::SqlWriteBindingError::Step(e)));

    unsafe {
        // Finalize destroys the prepared statement in SQLite
        sqlitex::libsqlite3_sys::sqlite3_finalize(preparred_statement_mut.stmt);
    }

    step_result?;                            }
                            }
                            Ok(())
                        })
                    }
                };
    Ok(MigrationsOutput {
        schema_init_method,
        watcher_tokens,
        all_tables,
    })
}
