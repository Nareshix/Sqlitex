use std::collections::HashMap;

use quote::quote;
use sqlitex_core::utility::utils::get_db_schema;
use sqlitex_type_inference::{table::create_tables, validate_cast_types, validate_create_table_types};

pub(crate) struct SchemaSourceOutput {
    pub schema_init_method: proc_macro2::TokenStream,
    pub open_connected_db_method: proc_macro2::TokenStream,
    pub watcher_tokens: proc_macro2::TokenStream,
    pub all_tables: HashMap<String, Vec<sqlitex_type_inference::table::ColumnInfo>>,
}

pub fn process_file_source(
    path: &syn::LitStr,
    db_path: &str,
) -> syn::Result<SchemaSourceOutput> {
    let mut schema_init_method = quote! {};
    let mut open_connected_db_method = quote! {};
    let mut all_tables = HashMap::new();

    let watcher_tokens = quote! { const _: &[u8] = include_bytes!(#db_path); };

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

        let filename = std::path::Path::new(db_path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(db_path);

        let doc_msg = format!(
            "Executes all SQL statements defined in the external `{}` file",
            filename
        );

        schema_init_method = quote! {
            #[doc = #doc_msg]
            pub fn init(&self) -> Result<(), sqlitex::errors::SqliteFailure> {
                self.__db.execute_batch(include_str!(#path))
            }
        };
    }

    let is_db_file = db_path.ends_with(".db")
        || db_path.ends_with(".sqlite")
        || db_path.ends_with(".sqlite3")
        || db_path.ends_with(".db3")
        || db_path.ends_with(".s3db")
        || db_path.ends_with(".sl3");

    if is_db_file {
        let file_name = std::path::Path::new(db_path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(db_path);

        let doc_msg = format!("Opens a connection to `{}`", file_name);

        open_connected_db_method = quote! {
            #[doc = #doc_msg]
            pub fn open_connected_db() -> Result<Self, sqlitex::errors::connection::SqliteOpenErrors> {
                let conn = sqlitex::internal_sqlite::sqlitex_connection::Connection::open(#path)?;
                Ok(Self::new(conn))
            }
        };
    }

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
        schema_init_method,
        open_connected_db_method,
        watcher_tokens,
        all_tables,
    })
}