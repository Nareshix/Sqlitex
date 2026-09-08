use std::collections::HashMap;

use crate::sqlite_validation::get_db_schema;
use quote::quote;
use sqlitex_type_inference::{table::create_tables, validate_cast_types, validate_create_table_types};

pub(crate) struct SchemaSourceOutput {
    pub watcher_tokens: proc_macro2::TokenStream,
    pub all_tables: HashMap<String, Vec<sqlitex_type_inference::table::ColumnInfo>>,
}

pub fn process_file_source(
    path: &syn::LitStr,
    db_path: &str,
) -> syn::Result<SchemaSourceOutput> {
    let mut all_tables = HashMap::new();
    let watcher_tokens = quote! { const _: &[u8] = include_bytes!(#db_path); };

    // If pointing to a .sql file, validate its syntax and types at compile time
    if db_path.ends_with(".sql") {
        let content = std::fs::read_to_string(db_path).map_err(|e| {
            syn::Error::new(path.span(), format!("Failed to read {}: {}", db_path, e))
        })?;

        validate_cast_types(&content).map_err(|msg| {
            syn::Error::new(path.span(), format!("In {}: {}", db_path, msg))
        })?;

        validate_create_table_types(&content).map_err(|msg| {
            syn::Error::new(path.span(), format!("In {}: {}", db_path, msg))
        })?;

        sqlitex_type_inference::validate_sql_file_syntax(&content).map_err(|msg| {
            syn::Error::new(path.span(), format!("In {}: {}", db_path, msg))
        })?;
    }

    // Load schema from SQLite in memory or live DB file
    let schemas = get_db_schema(db_path).map_err(|err| {
        syn::Error::new(path.span(), format!("Failed to load DB schema: {}", err))
    })?;

    for schema in schemas {
        validate_create_table_types(&schema).map_err(|msg| {
            syn::Error::new(path.span(), format!("In {}: {}", db_path, msg))
        })?;
        create_tables(&schema, &mut all_tables);
    }

    Ok(SchemaSourceOutput {
        watcher_tokens,
        all_tables,
    })
}