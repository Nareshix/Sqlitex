use proc_macro::TokenStream;
use syn::parse_macro_input;

mod config;
mod inline;
mod migrations;
mod schema_source;
mod sql_mapping;
mod sqlite_validation;
mod utils;

/// Compile-time validated SQL query returning an anonymous record struct.
///
/// Returns a specialized query object (`WriteQuery`, `ScalarQuery`, `OptionalQuery`, or `ManyQuery`)
/// based on query structure and constraints.
#[proc_macro]
pub fn query(input: TokenStream) -> TokenStream {
    let query_input = parse_macro_input!(input as inline::QueryInput);
    match inline::expand_query(query_input) {
        Ok(expanded) => expanded.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Compile-time validated SQL query mapping directly into an existing struct or primitive.
#[proc_macro]
pub fn query_as(input: TokenStream) -> TokenStream {
    let query_input = parse_macro_input!(input as inline::QueryInput);
    match inline::expand_query(query_input) {
        Ok(expanded) => expanded.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Standalone migration runner that executes all pending migrations from `sqlitex.toml`.
#[proc_macro]
pub fn migrate(input: TokenStream) -> TokenStream {
    let conn_expr = proc_macro2::TokenStream::from(input);
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("No MANIFEST_DIR");
    let (config_opt, _) = config::SqlitexConfig::load_from_manifest(&manifest_dir);

    let config = match config_opt {
        Some(c) => c,
        None => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                "sqlitex.toml not found. Configure [schema] path = 'migrations/' to use migrate!()",
            )
            .to_compile_error()
            .into();
        }
    };

    let schema_cfg = match config.schema {
        Some(s) => s,
        None => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                "[schema] path = 'migrations/' must be defined in sqlitex.toml to use migrate!()",
            )
            .to_compile_error()
            .into();
        }
    };

    let full_path = std::path::Path::new(&manifest_dir).join(&schema_cfg.path);
    let full_path_str = full_path.to_str().unwrap();
    let path_lit = syn::LitStr::new(full_path_str, proc_macro2::Span::call_site());

    match migrations::process_migrations_dir(&path_lit, full_path_str) {
        Ok(output) => {
            let schema_init = output.schema_init_method;
            let expanded = quote::quote! {
                {
                    struct __Migrator<'a> {
                        __db: &'a sqlitex::internal_sqlite::sqlitex_connection::Connection,
                    }
                    impl<'a> __Migrator<'a> {
                        #schema_init
                    }
                    let migrator = __Migrator { __db: #conn_expr };
                    migrator.migrate()
                }
            };
            expanded.into()
        }
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro_derive(SqlMapping)]
pub fn sql_mapping_derive(input: TokenStream) -> TokenStream {
    sql_mapping::expand_sql_mapping(input)
}