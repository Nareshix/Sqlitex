use std::{collections::HashMap, env, path::Path};

use proc_macro::TokenStream;
use quote::quote;
use sqlitex_core::utility::utils::{get_db_schema, validate_sql_syntax_with_sqlite};
use sqlitex_type_inference::{
    QueryCardinality, binding_patterns::get_type_of_binding_parameters, detect_query_cardinality,
    expr::BaseType, is_create_table, pg_cast_syntax_to_sqlite, rewrite_bool_columns,
    select_patterns::get_types_from_select, table::create_tables, validate_cast_types,
    validate_create_table_types, validate_insert_strict, validate_no_virtual_tables,
    validate_single_statement,
};
use syn::{
    Data, DeriveInput, Fields, Ident, ItemStruct, LitStr, Type, parse_macro_input, parse_quote,
    spanned::Spanned,
};

mod utils;
use utils::fnv1a_hash;
use utils::format_sql;

struct RuntimeSqlInput {
    return_type: Option<Type>,
    sql: syn::LitStr,
    args: Vec<Type>,
}

impl syn::parse::Parse for RuntimeSqlInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let return_type;
        let sql;

        if input.peek(syn::LitStr) {
            // sql_escape_hatch!("UPDATE...", arg, arg)
            return_type = None;
            sql = input.parse()?;
        } else {
            //  sql_escape_hatch!(UserDTO, "SELECT...", arg, arg)
            return_type = Some(input.parse()?);
            input.parse::<syn::Token![,]>()?; // Eat comma
            sql = input.parse()?;
        }

        let mut args = Vec::new();
        while !input.is_empty() {
            input.parse::<syn::Token![,]>()?; // Eat comma
            if input.is_empty() {
                break;
            }
            args.push(input.parse()?);
        }

        Ok(RuntimeSqlInput {
            return_type,
            sql,
            args,
        })
    }
}

fn parse_runtime_macro(ty: &syn::Type) -> syn::Result<Option<RuntimeSqlInput>> {
    if let syn::Type::Macro(type_macro) = ty
        && type_macro.mac.path.is_ident("sql_escape_hatch")
    {
        let parsed: RuntimeSqlInput = syn::parse2(type_macro.mac.tokens.clone())?;
        return Ok(Some(parsed));
    }
    Ok(None)
}

#[proc_macro_attribute]
pub fn sqlitex(args: TokenStream, input: TokenStream) -> TokenStream {
    let path_lit_opt = if args.is_empty() {
        None
    } else {
        match syn::parse::<syn::LitStr>(args) {
            Ok(lit) => {
                let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("No MANIFEST_DIR");
                let full_path = Path::new(&manifest_dir).join(lit.value());
                let full_path_str = full_path.to_str().expect("Invalid path string");

                Some(syn::LitStr::new(
                    full_path_str,
                    proc_macro2::Span::call_site(),
                ))
            }
            Err(_) => {
                let err = syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "sqlitex requires either no arguments or a path string to a sql/db file.",
                );
                let err_tokens = err.to_compile_error();
                let input_tokens = proc_macro2::TokenStream::from(input);
                return quote! {
                    #err_tokens
                    #input_tokens
                }
                .into();
            }
        }
    };

    let mut item_struct = parse_macro_input!(input as ItemStruct);

    match expand(&mut item_struct, path_lit_opt.as_ref()) {
        Ok((output, watcher)) => {
            let final_output = quote! {
                #output
                #watcher
            };

            final_output.into()
        }
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand(
    item_struct: &mut ItemStruct,
    db_path_lit: Option<&syn::LitStr>,
) -> syn::Result<(proc_macro2::TokenStream, proc_macro2::TokenStream)> {
    let mut all_tables = HashMap::new();

    let mut schema_init_method = quote! {};
    let mut open_connected_db_method = quote! {};
    let mut watcher_tokens = quote! {};

    if let Some(path) = db_path_lit {
        let db_path = path.value();
        let path_obj = std::path::Path::new(&db_path);

        if path_obj.is_dir() || db_path.ends_with('/') {
            let mut files: Vec<_> = std::fs::read_dir(path_obj)
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
                let checksum_tokens: proc_macro2::TokenStream =
                    checksum.to_string().parse().unwrap();
                migration_embeds.push(quote! {
                    (#version, #file_name, #checksum_tokens, include_str!(#file_path_str))
                });
            }

            watcher_tokens = quote! { #(#watcher_includes)* };

            // Boot up a real SQLite instance in the compiler and test the scripts file-by-file
            let schemas =
                sqlitex_core::utility::utils::get_db_schema_from_statements(&script_batches)
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
        } else {
            watcher_tokens = quote! { const _: &[u8] = include_bytes!(#db_path); };

            if db_path.ends_with(".sql") {
                let content = std::fs::read_to_string(&db_path).map_err(|e| {
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

                let filename = std::path::Path::new(&db_path)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(&db_path);

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
                let file_name = std::path::Path::new(&db_path)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(&db_path);

                let doc_msg = format!("Opens a connection to `{}`", file_name);

                open_connected_db_method = quote! {
                    #[doc = #doc_msg]
                    pub fn open_connected_db() -> Result<Self, sqlitex::errors::connection::SqliteOpenErrors> {
                        let conn = sqlitex::internal_sqlite::sqlitex_connection::Connection::open(#path)?;
                        Ok(Self::new(conn))
                    }
                };
            }

            let schemas = get_db_schema(&db_path).map_err(|err| {
                syn::Error::new(path.span(), format!("Failed to load DB schema: {}", err))
            })?;
            for schema in schemas {
                validate_create_table_types(&schema).map_err(|msg| {
                    syn::Error::new(path.span(), format!("In {}: {}", db_path, msg))
                })?;
                create_tables(&schema, &mut all_tables);
            }
        }
    }
    let struct_name = &item_struct.ident;

    let fields = match &mut item_struct.fields {
        syn::Fields::Named(named) => named,
        _ => {
            return Err(syn::Error::new(
                item_struct.span(),
                "sqlitex requires a struct with named fields",
            ));
        }
    };

    let mut sql_assignments = Vec::new();
    let mut standard_assignments = Vec::new();
    let mut standard_params = Vec::new();
    let mut generated_methods = Vec::new();
    let mut generated_structs = Vec::new();
    let mut re_exports = Vec::new();

    for field in fields.named.iter_mut() {
        let ident = field.ident.as_ref().unwrap();

        if ident == "transaction" {
            return Err(syn::Error::new(
                ident.span(),
                "`transaction` is a reserved keyword. Rename this field to something else.",
            ));
        }

        // `init` method is reserved when pointing to an external sql file
        if ident == "init"
            && db_path_lit.is_some()
            && db_path_lit.unwrap().value().ends_with(".sql")
        {
            return Err(syn::Error::new(
                ident.span(),
                "`init` is a reserved keyword when pointing to an external .sql file. Rename this field to something else.",
            ));
        }

        // `migrate` method is reserved when pointing to an external migrations folder
        if ident == "migrate"
            && db_path_lit.is_some()
            && (std::path::Path::new(&db_path_lit.unwrap().value()).is_dir()
                || db_path_lit.unwrap().value().ends_with('/'))
        {
            return Err(syn::Error::new(
                ident.span(),
                "`migrate` is a reserved keyword when pointing to an external migrations folder. Rename this field to something else.",
            ));
        }

        // `_bulk` is reserved for auto-generated batch methods
        let ident_str = ident.to_string();
        if ident_str.ends_with("_bulk") {
            let base_name = ident_str.strip_suffix("_bulk").unwrap();
            return Err(syn::Error::new(
                ident.span(),
                format!(
                    "`{}` has been reserved. This method is automatically generated for batch operations for `{}` method. Choose a different name.",
                    ident_str, base_name
                ),
            ));
        }

        let field_attrs = &field.attrs;

        // Check if type is sql!("...")
        if let Some(sql_lit) = parse_sql_macro_type(&field.ty)? {
            let sql_query = pg_cast_syntax_to_sqlite(&sql_lit.value());
            let sql_query = rewrite_bool_columns(&sql_query)
                .map_err(|msg| syn::Error::new(sql_lit.span(), msg))?;
            validate_no_virtual_tables(&sql_query)
                .map_err(|msg| syn::Error::new(sql_lit.span(), msg))?;
            validate_cast_types(&sql_query).map_err(|msg| syn::Error::new(sql_lit.span(), msg))?;

            validate_create_table_types(&sql_query)
                .map_err(|msg| syn::Error::new(sql_lit.span(), msg))?;

            if let Err(err_msg) = validate_single_statement(&sql_query) {
                return Err(syn::Error::new(sql_lit.span(), err_msg));
            }

            let transpiled_sql_lit = syn::LitStr::new(&sql_query, sql_lit.span());

            if let Err(err_msg) = validate_sql_syntax_with_sqlite(&all_tables, &sql_query) {
                return Err(syn::Error::new(sql_lit.span(), err_msg.to_string()));
            }

            if let Err(err_msg) = validate_insert_strict(&sql_query, &all_tables) {
                return Err(syn::Error::new(sql_lit.span(), err_msg.to_string()));
            }

            if is_create_table(&sql_query) {
                create_tables(&sql_query, &mut all_tables);

                field.ty = parse_quote!(sqlitex::internal_sqlite::sqlitex_statement::SqlitexStmt);
                sql_assignments.push(quote! {
                    #ident: sqlitex::internal_sqlite::sqlitex_statement::SqlitexStmt {
                        sql_query: #transpiled_sql_lit,
                        stmt: std::ptr::null_mut(),
                    }
                });

                let doc_comment = format!(" \n**SQL**\n```sql\n{}", format_sql(&sql_query));
                generated_methods.push(quote! {
                    #(#field_attrs)*
                    #[doc = #doc_comment]
                    pub fn #ident(&mut self) -> Result<(), sqlitex::errors::SqlWriteError> {
                        if self.#ident.stmt.is_null() {
                            unsafe {
                                sqlitex::utility::utils::prepare_stmt(
                                    self.__db.db,
                                    &mut self.#ident.stmt,
                                    self.#ident.sql_query
                                )?;
                            }
                        }
                        let mut preparred_statement = sqlitex::internal_sqlite::preparred_statement::PreparredStmt {
                            stmt: self.#ident.stmt,
                            conn: self.__db.db,
                        };
                        preparred_statement.step()?;
                        Ok(())
                    }
                });
                continue;
            }

            let select_types = match get_types_from_select(&sql_query, &all_tables) {
                Ok(types) => types,
                Err(err_msg) => {
                    return Err(syn::Error::new(
                        sql_lit.span(),
                        format!("Return Type Error: {}", err_msg),
                    ));
                }
            };

            let binding_types = match get_type_of_binding_parameters(&sql_query, &all_tables) {
                Ok(types) => types,
                Err(err) => {
                    let lines: Vec<&str> = sql_query.lines().collect();
                    let line_idx = err.start.line.saturating_sub(1) as usize;
                    let start_col = err.start.column.saturating_sub(1) as usize;
                    let end_col = err.end.column.saturating_sub(1) as usize;
                    let mut msg = err.message.to_string();
                    if let Some(raw_line) = lines.get(line_idx) {
                        let indent_len_bytes = raw_line
                            .char_indices()
                            .take_while(|(_, c)| c.is_whitespace())
                            .last()
                            .map(|(i, c)| i + c.len_utf8())
                            .unwrap_or(0);
                        let start_byte_idx = raw_line
                            .chars()
                            .take(start_col)
                            .map(|c| c.len_utf8())
                            .sum::<usize>();
                        let end_byte_idx = raw_line
                            .chars()
                            .take(end_col)
                            .map(|c| c.len_utf8())
                            .sum::<usize>();
                        let safe_indent = if indent_len_bytes <= start_byte_idx {
                            indent_len_bytes
                        } else {
                            0
                        };
                        let trimmed_line = &raw_line[safe_indent..];
                        let err_start_in_trimmed = start_byte_idx - safe_indent;
                        let err_len = end_byte_idx - start_byte_idx;
                        let padding: String = trimmed_line[..err_start_in_trimmed]
                            .chars()
                            .map(|c| if c == '\t' { '\t' } else { ' ' })
                            .collect();
                        let arrows = "^".repeat(err_len.max(1));
                        msg = format!("{}\n\n{}\n{}{}", msg, trimmed_line, padding, arrows);
                    }
                    return Err(syn::Error::new(sql_lit.span(), msg));
                }
            };

            let mut param_names = Vec::new();
            let mut used_names = std::collections::HashSet::new();
            for (i, param) in binding_types.iter().enumerate() {
                let mut base_name = param.name.clone();
                if base_name == "arg" || base_name.is_empty() {
                    base_name = format!("arg_{}", i);
                }

                // Convert invalid Rust identifier characters to underscores
                base_name = base_name.replace(|c: char| !c.is_ascii_alphanumeric(), "_");

                // Prevent starting with a number
                if base_name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    base_name = format!("arg_{}", base_name);
                }

                // Keep name valid for Rust
                let is_keyword = matches!(
                    base_name.as_str(),
                    "type"
                        | "match"
                        | "let"
                        | "fn"
                        | "struct"
                        | "enum"
                        | "trait"
                        | "impl"
                        | "where"
                        | "for"
                        | "loop"
                        | "while"
                        | "if"
                        | "else"
                        | "return"
                        | "break"
                        | "continue"
                        | "mut"
                        | "ref"
                        | "in"
                        | "as"
                        | "use"
                        | "pub"
                        | "const"
                        | "static"
                        | "move"
                        | "async"
                        | "await"
                        | "dyn"
                        | "self"
                        | "super"
                        | "crate"
                );
                if is_keyword {
                    base_name = format!("{}_arg", base_name);
                }

                // Deduplicate names: id, id_1, id_2 etc.
                let mut final_name = base_name.clone();
                let mut counter = 1;
                while used_names.contains(&final_name) {
                    final_name = format!("{}_{}", base_name, counter);
                    counter += 1;
                }
                used_names.insert(final_name.clone());
                param_names.push(final_name);
            }

            let doc_comment = format!(" \n**SQL**\n```sql\n{}", format_sql(&sql_lit.value()));

            field.ty = parse_quote!(sqlitex::internal_sqlite::sqlitex_statement::SqlitexStmt);

            sql_assignments.push(quote! {
                #ident: sqlitex::internal_sqlite::sqlitex_statement::SqlitexStmt {
                    sql_query: #transpiled_sql_lit,
                    stmt: std::ptr::null_mut(),
                }
            });

            if select_types.is_empty() && binding_types.is_empty() {
                generated_methods.push(quote! {
                    #(#field_attrs)*
                    #[doc = #doc_comment]
                    pub fn #ident(&mut self) -> Result<(), sqlitex::errors::SqlWriteError> {
                        if self.#ident.stmt.is_null() {
                            unsafe {
                                sqlitex::utility::utils::prepare_stmt(
                                    self.__db.db,
                                    &mut self.#ident.stmt,
                                    self.#ident.sql_query
                                )?;
                            }
                        }
                        let mut preparred_statement = sqlitex::internal_sqlite::preparred_statement::PreparredStmt {
                            stmt: self.#ident.stmt,
                            conn: self.__db.db,
                        };
                        preparred_statement.step()?;
                        Ok(())
                    }
                });
            } else if select_types.is_empty() && !binding_types.is_empty() {
                let mut method_args = Vec::new();
                let mut bind_calls = Vec::new();

                for (i, bind_param) in binding_types.iter().enumerate() {
                    let arg_name = quote::format_ident!("{}", param_names[i]);
                    let bind_type = &bind_param.data_type;
                    let bind_index = (i + 1) as i32;

                    let rust_base_type = match bind_type.base_type {
                        BaseType::Integer => quote! { i64 },
                        BaseType::Real => quote! { f64 },
                        BaseType::Bool => quote! { bool },
                        BaseType::Text => quote! { &str },
                        BaseType::Blob => quote! { &[u8] },
                        _ => {
                            return Err(syn::Error::new(
                                sql_lit.span(),
                                "Unable to infer type for `?`. Consider casting with `::` or `CAST AS`",
                            ));
                        }
                    };

                    let final_type = if bind_type.nullable {
                        quote! { Option<#rust_base_type> }
                    } else {
                        quote! { #rust_base_type }
                    };

                    method_args.push(quote! { #arg_name: #final_type });

                    bind_calls.push(quote! {
                        preparred_statement.bind_parameter(#bind_index, #arg_name)?;
                    });
                }

                generated_methods.push(quote! {
                    #(#field_attrs)*
                    #[doc = #doc_comment]
                    pub fn #ident(&mut self, #(#method_args),*) -> Result<(), sqlitex::errors::SqlWriteBindingError> {
                        if self.#ident.stmt.is_null() {
                            unsafe {
                                sqlitex::utility::utils::prepare_stmt(
                                    self.__db.db,
                                    &mut self.#ident.stmt,
                                    self.#ident.sql_query
                                )?;
                            }
                        }

                        let mut preparred_statement = sqlitex::internal_sqlite::preparred_statement::PreparredStmt {
                            stmt: self.#ident.stmt,
                            conn: self.__db.db,
                        };

                        #(#bind_calls)*

                        preparred_statement.step()?;

                        Ok(())
                    }
                });

                // generate _bulk method
                let many_ident = quote::format_ident!("{}_bulk", ident);

                let mut many_owned_types = Vec::new();
                let mut many_bind_calls = Vec::new();

                for (i, bind_param) in binding_types.iter().enumerate() {
                    let bind_index = (i + 1) as i32;
                    let tuple_idx = syn::Index::from(i);
                    let bind_type = &bind_param.data_type;

                    let owned_base_type = match bind_type.base_type {
                        BaseType::Integer => quote! { i64 },
                        BaseType::Real => quote! { f64 },
                        BaseType::Bool => quote! { bool },
                        BaseType::Text => quote! { String },
                        BaseType::Blob => quote! { Vec<u8> },
                        _ => {
                            return Err(syn::Error::new(
                                sql_lit.span(),
                                "Unable to infer type for `?`. Consider casting with `::` or `CAST AS`",
                            ));
                        }
                    };

                    let owned_final_type = if bind_type.nullable {
                        quote! { Option<#owned_base_type> }
                    } else {
                        quote! { #owned_base_type }
                    };

                    many_owned_types.push(owned_final_type);

                    let bind_expr = if bind_type.nullable {
                        match bind_type.base_type {
                            BaseType::Text => quote! { item.#tuple_idx.as_deref() },
                            BaseType::Blob => quote! { item.#tuple_idx.as_deref() },
                            _ => quote! { item.#tuple_idx },
                        }
                    } else {
                        match bind_type.base_type {
                            BaseType::Text => quote! { item.#tuple_idx.as_str() },
                            BaseType::Blob => quote! { item.#tuple_idx.as_slice() },
                            _ => quote! { item.#tuple_idx },
                        }
                    };

                    many_bind_calls.push(quote! {
                        if let Err(__e) = preparred_statement.bind_parameter(#bind_index, #bind_expr) {
                            if is_outermost {
                                let _ = self.__db.execute_batch("ROLLBACK");
                            } else {
                                let _ = self.__db.execute_batch("ROLLBACK TO SAVEPOINT sqlitex_batch");
                                let _ = self.__db.execute_batch("RELEASE SAVEPOINT sqlitex_batch");
                            }
                            return Err(sqlitex::errors::Error::from(sqlitex::errors::SqlWriteBindingError::Bind(__e)));
                        }
                    });
                }

                let (item_type, final_bulk_bind_calls) = if binding_types.len() == 1 {
                    let bind_type = &binding_types[0].data_type;
                    let single_bind_expr = if bind_type.nullable {
                        match bind_type.base_type {
                            BaseType::Text => quote! { item.as_deref() },
                            BaseType::Blob => quote! { item.as_deref() },
                            _ => quote! { *item },
                        }
                    } else {
                        match bind_type.base_type {
                            BaseType::Text => quote! { item.as_str() },
                            BaseType::Blob => quote! { item.as_slice() },
                            _ => quote! { *item },
                        }
                    };

                    let single_call = quote! {
                        if let Err(__e) = preparred_statement.bind_parameter(1, #single_bind_expr) {
                            if is_outermost {
                                let _ = self.__db.execute_batch("ROLLBACK");
                            } else {
                                let _ = self.__db.execute_batch("ROLLBACK TO SAVEPOINT sqlitex_batch");
                                let _ = self.__db.execute_batch("RELEASE SAVEPOINT sqlitex_batch");
                            }
                            return Err(sqlitex::errors::Error::from(sqlitex::errors::SqlWriteBindingError::Bind(__e)));
                        }
                    };
                    (many_owned_types[0].clone(), vec![single_call])
                } else {
                    (quote! { (#(#many_owned_types),*) }, many_bind_calls)
                };

                let many_doc_header: String = format!(
                    r#"This is a batch operation version of [`{}`].
Prefer this when inserting, updating, or deleting multiple rows at once for better performance.

This operation is atomic and if you need more precise control over batching, use [`transaction`].

# Example

```rust, ignore
let bulk = [
    (0.0, "Alice".to_string(), true),
    (1.0, "Bob".to_string(), false),
    (2.0, "Charlie".to_string(), true),
];

db.{}_bulk(&bulk)?;
```"#,
                    ident, ident
                );

                generated_methods.push(quote! {
                    #(#field_attrs)*
                    #[doc = #many_doc_header]
                    #[doc = #doc_comment]
                    pub fn #many_ident(&mut self, items: &[#item_type]) -> Result<(), sqlitex::errors::Error> {
                        if items.is_empty() {
                            return Ok(());
                        }

                        if self.#ident.stmt.is_null() {
                            unsafe {
                                sqlitex::utility::utils::prepare_stmt(
                                    self.__db.db,
                                    &mut self.#ident.stmt,
                                    self.#ident.sql_query
                                ).map_err(|e| sqlitex::errors::Error::from(sqlitex::errors::SqlWriteBindingError::Prepare(e)))?;
                            }
                        }

                        let is_outermost = unsafe { sqlitex::libsqlite3_sys::sqlite3_get_autocommit(self.__db.db) != 0 };

                        if is_outermost {
                            self.__db.execute_batch("BEGIN IMMEDIATE").map_err(sqlitex::errors::Error::from)?;
                        } else {
                            self.__db.execute_batch("SAVEPOINT sqlitex_batch").map_err(sqlitex::errors::Error::from)?;
                        }

                        for item in items {
                            let mut preparred_statement = sqlitex::internal_sqlite::preparred_statement::PreparredStmt {
                                stmt: self.#ident.stmt,
                                conn: self.__db.db,
                            };

                            #(#final_bulk_bind_calls)*

                            if let Err(__e) = preparred_statement.step() {
                                if is_outermost {
                                    let _ = self.__db.execute_batch("ROLLBACK");
                                } else {
                                    let _ = self.__db.execute_batch("ROLLBACK TO SAVEPOINT sqlitex_batch");
                                    let _ = self.__db.execute_batch("RELEASE SAVEPOINT sqlitex_batch");
                                }
                                return Err(sqlitex::errors::Error::from(sqlitex::errors::SqlWriteBindingError::Step(__e)));
                            }
                        }

                        if is_outermost {
                            if let Err(__e) = self.__db.execute_batch("COMMIT") {
                                let _ = self.__db.execute_batch("ROLLBACK");
                                return Err(sqlitex::errors::Error::from(__e));
                            }
                        } else {
                            if let Err(__e) = self.__db.execute_batch("RELEASE SAVEPOINT sqlitex_batch") {
                                let _ = self.__db.execute_batch("ROLLBACK TO SAVEPOINT sqlitex_batch");
                                let _ = self.__db.execute_batch("RELEASE SAVEPOINT sqlitex_batch");
                                return Err(sqlitex::errors::Error::from(__e));
                            }
                        }

                        Ok(())
                    }
                });
            } else if !select_types.is_empty() && binding_types.is_empty() {
                let cardinality = detect_query_cardinality(&sql_query, &all_tables);
                let is_single_col = select_types.len() == 1;

                let method_name = ident.to_string();
                let pascal_name: String = method_name
                    .split('_')
                    .map(|s| {
                        let mut c = s.chars();
                        match c.next() {
                            None => String::new(),
                            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                        }
                    })
                    .collect();

                let output_struct_name = quote::format_ident!("{}", pascal_name);
                let mapper_struct_name = quote::format_ident!("{}_", pascal_name);
                let scalar_mapper_name = quote::format_ident!("{}_scalar_", ident);

                // Build the primitive type for single-col scalar path
                let single_col_rust_type = if is_single_col {
                    let col = &select_types[0];
                    let base_ty = match col.data_type.base_type {
                        BaseType::Integer => quote! { i64 },
                        BaseType::Real => quote! { f64 },
                        BaseType::Text => quote! { String },
                        BaseType::Blob => quote! { Vec<u8> },
                        BaseType::Bool => quote! { bool },
                        _ => quote! { i64 }, // fallback for e.g. COUNT(*)
                    };
                    if col.data_type.nullable {
                        quote! { Option<#base_ty> }
                    } else {
                        quote! { #base_ty }
                    }
                } else {
                    quote! {}
                };

                // Always generate the named struct (needed for multi-col and as fallback)
                if !is_single_col || cardinality == QueryCardinality::MaybeMany {
                    re_exports.push(output_struct_name.clone());

                    let mut struct_fields = Vec::new();
                    for col in select_types.iter() {
                        let name = quote::format_ident!("{}", col.name);
                        let base_ty = match col.data_type.base_type {
                            BaseType::Integer => quote! { i64 },
                            BaseType::Real => quote! { f64 },
                            BaseType::Text => quote! { String },
                            BaseType::Blob => quote! { Vec<u8> },
                            BaseType::Bool => quote! { bool },
                            _ => {
                                return Err(syn::Error::new(
                                    sql_lit.span(),
                                    "Unable to infer return type. Consider casting with `::` or `CAST AS`",
                                ));
                            }
                        };
                        let final_ty = if col.data_type.nullable {
                            quote! { Option<#base_ty> }
                        } else {
                            quote! { #base_ty }
                        };
                        struct_fields.push(quote! { pub #name: #final_ty });
                    }
                    generated_structs.push(quote! {
                        #[derive(Clone, Debug, sqlitex::SqlMapping)]
                        pub struct #output_struct_name {
                            #(#struct_fields),*
                        }
                    });
                }

                // For single-col smart paths, generate a private scalar mapper
                if is_single_col && cardinality != QueryCardinality::MaybeMany {
                    let col = &select_types[0];
                    let is_nullable = col.data_type.nullable;
                    let base_ty = match col.data_type.base_type {
                        BaseType::Integer => quote! { i64 },
                        BaseType::Real => quote! { f64 },
                        BaseType::Text => quote! { String },
                        BaseType::Blob => quote! { Vec<u8> },
                        BaseType::Bool => quote! { bool },
                        _ => quote! { i64 },
                    };
                    if is_nullable {
                        // SUM/AVG/MIN/MAX: always one row but value may be NULL
                        // Output = Option<T> so FromSql handles NULL correctly
                        generated_structs.push(quote! {
            #[allow(non_camel_case_types)]
            #[derive(Clone)]
            struct #scalar_mapper_name;
            impl sqlitex::traits::row_mapper::RowMapper for #scalar_mapper_name {
                type Output = Option<#base_ty>;
                unsafe fn map_row(&self, stmt: *mut sqlitex::libsqlite3_sys::sqlite3_stmt) -> Option<#base_ty> {
                    <Option<#base_ty> as sqlitex::traits::from_sql::FromSql>::from_sql(stmt, 0)
                }
            }
        });
                    } else {
                        // COUNT: always one row, never NULL
                        generated_structs.push(quote! {
            #[allow(non_camel_case_types)]
            #[derive(Clone)]
            struct #scalar_mapper_name;
            impl sqlitex::traits::row_mapper::RowMapper for #scalar_mapper_name {
                type Output = #base_ty;
                unsafe fn map_row(&self, stmt: *mut sqlitex::libsqlite3_sys::sqlite3_stmt) -> #base_ty {
                    <#base_ty as sqlitex::traits::from_sql::FromSql>::from_sql(stmt, 0)
                }
            }
        });
                    }
                }
                let prepare_block = quote! {
                    if self.#ident.stmt.is_null() {
                        unsafe {
                            sqlitex::utility::utils::prepare_stmt(
                                self.__db.db,
                                &mut self.#ident.stmt,
                                self.#ident.sql_query
                            )?;
                        }
                    }
                    let preparred_statement = sqlitex::internal_sqlite::preparred_statement::PreparredStmt {
                        stmt: self.#ident.stmt,
                        conn: self.__db.db,
                    };
                };

                match (cardinality, is_single_col) {
                    (QueryCardinality::MaybeMany, _) => {
                        generated_methods.push(quote! {
                #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self) -> Result<sqlitex::internal_sqlite::rows_dao::Rows<'_, #mapper_struct_name>, sqlitex::errors::SqlReadError> {
                    #prepare_block
                    Ok(preparred_statement.query(#output_struct_name))
                }
            });
                    }
                    (QueryCardinality::ZeroOrOne, false) => {
                        generated_methods.push(quote! {
                #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self) -> Result<Option<#output_struct_name>, sqlitex::errors::Error> {
                    #prepare_block
                    preparred_statement.query(#output_struct_name)
                        .first()
                        .map_err(sqlitex::errors::Error::from)
                }
            });
                    }
                    (QueryCardinality::ExactlyOne, false) => {
                        generated_methods.push(quote! {
                #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self) -> Result<#output_struct_name, sqlitex::errors::Error> {
                    #prepare_block
                    preparred_statement.query(#output_struct_name)
                        .first()
                        .map_err(sqlitex::errors::Error::from)
                        .map(|opt| opt.expect("aggregate query must return exactly one row"))
                }
            });
                    }
                    (QueryCardinality::ZeroOrOne, true) => {
                        generated_methods.push(quote! {
                #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self) -> Result<Option<#single_col_rust_type>, sqlitex::errors::Error> {
                    #prepare_block
                    preparred_statement.query(#scalar_mapper_name)
                        .first()
                        .map_err(sqlitex::errors::Error::from)
                }
            });
                    }
                    (QueryCardinality::ExactlyOne, true) => {
                        generated_methods.push(quote! {
                #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self) -> Result<#single_col_rust_type, sqlitex::errors::Error> {
                    #prepare_block
                    preparred_statement.query(#scalar_mapper_name)
                        .first()
                        .map_err(sqlitex::errors::Error::from)
                        .map(|opt| opt.expect("aggregate query must return exactly one row"))
                }
            });
                    }
                }
            } else {
                let cardinality = detect_query_cardinality(&sql_query, &all_tables);
                let is_single_col = select_types.len() == 1;

                let method_name = ident.to_string();
                let pascal_name: String = method_name
                    .split('_')
                    .map(|s| {
                        let mut c = s.chars();
                        match c.next() {
                            None => String::new(),
                            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                        }
                    })
                    .collect();

                let output_struct_name = quote::format_ident!("{}", pascal_name);
                let mapper_struct_name = quote::format_ident!("{}_", pascal_name);
                let scalar_mapper_name = quote::format_ident!("{}_scalar_", ident);

                // Build method args and bind calls (same as before)
                let mut method_args = Vec::new();
                let mut bind_calls = Vec::new();

                for (i, bind_param) in binding_types.iter().enumerate() {
                    let arg_name = quote::format_ident!("{}", param_names[i]);
                    let bind_type = &bind_param.data_type;
                    let bind_index = (i + 1) as i32;

                    let rust_base_type = match bind_type.base_type {
                        BaseType::Integer => quote! { i64 },
                        BaseType::Real => quote! { f64 },
                        BaseType::Bool => quote! { bool },
                        BaseType::Text => quote! { &str },
                        BaseType::Blob => quote! { &[u8] },
                        _ => {
                            return Err(syn::Error::new(
                                sql_lit.span(),
                                "Unable to infer type for `?`. Consider casting with `::` or `CAST AS`",
                            ));
                        }
                    };

                    let final_type = if bind_type.nullable {
                        quote! { Option<#rust_base_type> }
                    } else {
                        quote! { #rust_base_type }
                    };

                    method_args.push(quote! { #arg_name: #final_type });
                    bind_calls.push(quote! {
                        preparred_statement.bind_parameter(#bind_index, #arg_name)?;
                    });
                }

                // Build the primitive type for single-col scalar path
                let single_col_rust_type = if is_single_col {
                    let col = &select_types[0];
                    let base_ty = match col.data_type.base_type {
                        BaseType::Integer => quote! { i64 },
                        BaseType::Real => quote! { f64 },
                        BaseType::Text => quote! { String },
                        BaseType::Blob => quote! { Vec<u8> },
                        BaseType::Bool => quote! { bool },
                        _ => quote! { i64 },
                    };
                    if col.data_type.nullable {
                        quote! { Option<#base_ty> }
                    } else {
                        quote! { #base_ty }
                    }
                } else {
                    quote! {}
                };

                // Generate named struct for multi-col or MaybeMany
                if !is_single_col || cardinality == QueryCardinality::MaybeMany {
                    re_exports.push(output_struct_name.clone());

                    let mut struct_fields = Vec::new();
                    for col in select_types.iter() {
                        let name = quote::format_ident!("{}", col.name);
                        let base_ty = match col.data_type.base_type {
                            BaseType::Integer => quote! { i64 },
                            BaseType::Real => quote! { f64 },
                            BaseType::Text => quote! { String },
                            BaseType::Blob => quote! { Vec<u8> },
                            BaseType::Bool => quote! { bool },
                            _ => {
                                return Err(syn::Error::new(
                                    sql_lit.span(),
                                    "Unable to infer return type for this expression. Consider casting with `::` or `CAST AS`",
                                ));
                            }
                        };
                        let final_ty = if col.data_type.nullable {
                            quote! { Option<#base_ty> }
                        } else {
                            quote! { #base_ty }
                        };
                        struct_fields.push(quote! { pub #name: #final_ty });
                    }
                    generated_structs.push(quote! {
                        #[derive(Clone, Debug, sqlitex::SqlMapping)]
                        pub struct #output_struct_name {
                            #(#struct_fields),*
                        }
                    });
                }

                // Generate scalar mapper for single-col smart paths
                if is_single_col && cardinality != QueryCardinality::MaybeMany {
                    let col = &select_types[0];
                    let is_nullable = col.data_type.nullable;
                    let base_ty = match col.data_type.base_type {
                        BaseType::Integer => quote! { i64 },
                        BaseType::Real => quote! { f64 },
                        BaseType::Text => quote! { String },
                        BaseType::Blob => quote! { Vec<u8> },
                        BaseType::Bool => quote! { bool },
                        _ => quote! { i64 },
                    };
                    if is_nullable {
                        // SUM/AVG/MIN/MAX: always one row but value may be NULL
                        // Output = Option<T> so FromSql handles NULL correctly
                        generated_structs.push(quote! {
            #[allow(non_camel_case_types)]
            #[derive(Clone)]
            struct #scalar_mapper_name;
            impl sqlitex::traits::row_mapper::RowMapper for #scalar_mapper_name {
                type Output = Option<#base_ty>;
                unsafe fn map_row(&self, stmt: *mut sqlitex::libsqlite3_sys::sqlite3_stmt) -> Option<#base_ty> {
                    <Option<#base_ty> as sqlitex::traits::from_sql::FromSql>::from_sql(stmt, 0)
                }
            }
        });
                    } else {
                        // COUNT: always one row, never NULL
                        generated_structs.push(quote! {
            #[allow(non_camel_case_types)]
            #[derive(Clone)]
            struct #scalar_mapper_name;
            impl sqlitex::traits::row_mapper::RowMapper for #scalar_mapper_name {
                type Output = #base_ty;
                unsafe fn map_row(&self, stmt: *mut sqlitex::libsqlite3_sys::sqlite3_stmt) -> #base_ty {
                    <#base_ty as sqlitex::traits::from_sql::FromSql>::from_sql(stmt, 0)
                }
            }
        });
                    }
                }
                // prepare_block includes mut and bind calls
                let prepare_block = quote! {
                    if self.#ident.stmt.is_null() {
                        unsafe {
                            sqlitex::utility::utils::prepare_stmt(
                                self.__db.db,
                                &mut self.#ident.stmt,
                                self.#ident.sql_query
                            )?;
                        }
                    }
                    let mut preparred_statement = sqlitex::internal_sqlite::preparred_statement::PreparredStmt {
                        stmt: self.#ident.stmt,
                        conn: self.__db.db,
                    };
                    #(#bind_calls)*
                };

                match (cardinality, is_single_col) {
                    (QueryCardinality::MaybeMany, _) => {
                        generated_methods.push(quote! {
                            #(#field_attrs)*
                            #[doc = #doc_comment]
                            pub fn #ident(&mut self, #(#method_args),*) -> Result<sqlitex::internal_sqlite::rows_dao::Rows<'_, #mapper_struct_name>, sqlitex::errors::SqlReadErrorBindings> {
                                #prepare_block
                                Ok(preparred_statement.query(#output_struct_name))
                            }
                        });
                    }
                    (QueryCardinality::ZeroOrOne, false) => {
                        generated_methods.push(quote! {
                            #(#field_attrs)*
                            #[doc = #doc_comment]
                            pub fn #ident(&mut self, #(#method_args),*) -> Result<Option<#output_struct_name>, sqlitex::errors::Error> {
                                #prepare_block
                                preparred_statement.query(#output_struct_name)
                                    .first()
                                    .map_err(sqlitex::errors::Error::from)
                            }
                        });
                    }
                    (QueryCardinality::ExactlyOne, false) => {
                        generated_methods.push(quote! {
                            #(#field_attrs)*
                            #[doc = #doc_comment]
                            pub fn #ident(&mut self, #(#method_args),*) -> Result<#output_struct_name, sqlitex::errors::Error> {
                                #prepare_block
                                preparred_statement.query(#output_struct_name)
                                    .first()
                                    .map_err(sqlitex::errors::Error::from)
                                    .map(|opt| opt.expect("aggregate query must return exactly one row"))
                            }
                        });
                    }
                    (QueryCardinality::ZeroOrOne, true) => {
                        generated_methods.push(quote! {
                            #(#field_attrs)*
                            #[doc = #doc_comment]
                            pub fn #ident(&mut self, #(#method_args),*) -> Result<Option<#single_col_rust_type>, sqlitex::errors::Error> {
                                #prepare_block
                                preparred_statement.query(#scalar_mapper_name)
                                    .first()
                                    .map_err(sqlitex::errors::Error::from)
                            }
                        });
                    }
                    (QueryCardinality::ExactlyOne, true) => {
                        generated_methods.push(quote! {
                            #(#field_attrs)*
                            #[doc = #doc_comment]
                            pub fn #ident(&mut self, #(#method_args),*) -> Result<#single_col_rust_type, sqlitex::errors::Error> {
                                #prepare_block
                                preparred_statement.query(#scalar_mapper_name)
                                    .first()
                                    .map_err(sqlitex::errors::Error::from)
                                    .map(|opt| opt.expect("aggregate query must return exactly one row"))
                            }
                        });
                    }
                }
            }
        } else if let Some(runtime_input) = parse_runtime_macro(&field.ty)? {
            let sql_lit = runtime_input.sql;
            let sql_query = pg_cast_syntax_to_sqlite(&sql_lit.value());
            let sql_query = rewrite_bool_columns(&sql_query)
                .map_err(|msg| syn::Error::new(sql_lit.span(), msg))?;

            let transpiled_sql_lit = syn::LitStr::new(&sql_query, sql_lit.span());

            field.ty = parse_quote!(sqlitex::internal_sqlite::sqlitex_statement::SqlitexStmt);

            sql_assignments.push(quote! {
                #ident: sqlitex::internal_sqlite::sqlitex_statement::SqlitexStmt {
                    sql_query: #transpiled_sql_lit,
                    stmt: std::ptr::null_mut(),
                }
            });

            let mut method_args = Vec::new();
            let mut bind_calls = Vec::new();

            for (i, arg_type) in runtime_input.args.iter().enumerate() {
                let arg_name = quote::format_ident!("arg_{}", i);
                let bind_index = (i + 1) as i32;

                method_args.push(quote! { #arg_name: #arg_type });

                bind_calls.push(quote! {
                    preparred_statement.bind_parameter(#bind_index, #arg_name)?;
                });
            }

            let doc_comment = format!(" \n**SQL**\n```sql\n{}", format_sql(&sql_lit.value()));

            if let Some(ret_type) = runtime_input.return_type {
                let mapper_type = if let syn::Type::Path(type_path) = &ret_type {
                    if let Some(segment) = type_path.path.segments.last() {
                        let type_name = segment.ident.to_string();
                        let primitives = [
                            "i64", "i32", "u64", "u32", "f64", "f32", "bool", "String", "Option",
                        ];

                        if primitives.iter().any(|&p| type_name.starts_with(p)) {
                            quote! { #ret_type }
                        } else {
                            let new_ident = quote::format_ident!("{}_", segment.ident);
                            quote! { #new_ident }
                        }
                    } else {
                        quote! { #ret_type }
                    }
                } else {
                    quote! { #ret_type }
                };

                generated_methods.push(quote! {
                    #(#field_attrs)*
                    #[doc = #doc_comment]
                    // SELECT
                    pub fn #ident(&mut self, #(#method_args),*) -> Result<sqlitex::internal_sqlite::rows_dao::Rows<#mapper_type>, sqlitex::errors::SqlReadErrorBindings> {
                        if self.#ident.stmt.is_null() {
                            unsafe {
                                sqlitex::utility::utils::prepare_stmt(
                                    self.__db.db,
                                    &mut self.#ident.stmt,
                                    self.#ident.sql_query
                                )?;
                            }
                        }

                        let mut preparred_statement = sqlitex::internal_sqlite::preparred_statement::PreparredStmt {
                            stmt: self.#ident.stmt,
                            conn: self.__db.db,
                        };

                        #(#bind_calls)*

                        Ok(preparred_statement.query(#mapper_type))
                    }
                });
            } else {
                // Non SELECT
                generated_methods.push(quote! {
                    #(#field_attrs)*
                    #[doc = #doc_comment]
                    pub fn #ident(&mut self, #(#method_args),*) -> Result<(), sqlitex::errors::SqlWriteBindingError> {
                        if self.#ident.stmt.is_null() {
                            unsafe {
                                sqlitex::utility::utils::prepare_stmt(
                                    self.__db.db,
                                    &mut self.#ident.stmt,
                                    self.#ident.sql_query
                                )?;
                            }
                        }

                        let mut preparred_statement = sqlitex::internal_sqlite::preparred_statement::PreparredStmt {
                            stmt: self.#ident.stmt,
                            conn: self.__db.db,
                        };

                        #(#bind_calls)*

                        preparred_statement.step()?;
                        Ok(())
                    }
                });
            }
        }
        // normal struct and field
        else {
            let ty = &field.ty;
            standard_params.push(quote! { #ident: #ty });
            standard_assignments.push(quote! { #ident });
        }
    }

    fields.named.push(
        parse_quote! { __db: std::sync::Arc<sqlitex::internal_sqlite::sqlitex_connection::Connection> }
    );

    let (impl_generics, ty_generics, where_clause) = item_struct.generics.split_for_impl();

    //
    item_struct.vis = parse_quote!(pub);

    let transaction_doc = r#"Executes multiple database operations inside a single transaction.

If the closure returns `Ok`, the transaction is committed.

If the closure returns `Err`, the transaction is rolled back.

# Example

```rust, ignore
db.transaction(|tx| {
    tx.insert_user("Alice")?;
    tx.insert_post("Hello")?;
    Ok(())
})?;"#;

    Ok((
        quote! {
                    #(#generated_structs)*
                    #item_struct

                    const _: () = {
                        impl #impl_generics #struct_name #ty_generics #where_clause {
                        /// Creates a new instance.
                        ///
                        /// To share one connection across multiple structs, clone the `Arc`:
                        ///
                        /// # Example
                        /// ```rust,ignore
                        /// let conn = Connection::open("app.db")?; // the conn is the arc btw
                        ///
                        /// let mut users = UsersDb::new(conn.clone());
                        /// let mut logs  = LogsDb::new(conn.clone());
                        /// let mut posts = PostsDb::new(conn); // last one doesn't need clone
                        /// ```
                        ///
                        /// `Arc::clone` does not duplicate the connection. All structs point to the same database.
                        pub fn new(
                            db: impl Into<std::sync::Arc<sqlitex::internal_sqlite::sqlitex_connection::Connection>>,
                            #(#standard_params),*
                        ) -> Self {
                            Self {
                                __db: db.into(), // Call .into() to turn it into the Arc
                                #(#standard_assignments,)*
                                #(#sql_assignments,)*
                                }
                            }


        #[doc = #transaction_doc]
        pub fn transaction<T, F>(&mut self, f: F) -> Result<T, sqlitex::errors::Error>
        where
            F: FnOnce(&mut Self) -> Result<T, sqlitex::errors::Error>,
        {
            // Check if we are the outermost transaction
            let is_outermost = unsafe {
                sqlitex::libsqlite3_sys::sqlite3_get_autocommit(self.__db.db) != 0
            };

            if is_outermost {
                self.__db.execute_batch("BEGIN IMMEDIATE").map_err(sqlitex::errors::Error::from)?;
            } else {
                self.__db.execute_batch("SAVEPOINT sqlitex_tx").map_err(sqlitex::errors::Error::from)?;
            }

            // Drop on panic unwind
            let db_ref = self.__db.clone();
            struct RollbackGuard {
                db: std::sync::Arc<sqlitex::internal_sqlite::sqlitex_connection::Connection>,
                is_outermost: bool,
                committed: bool,
            }

            impl Drop for RollbackGuard {
                fn drop(&mut self) {
                    if !self.committed {
                        if self.is_outermost {
                            let _ = self.db.execute_batch("ROLLBACK");
                        } else {
                            let _ = self.db.execute_batch("ROLLBACK TO SAVEPOINT sqlitex_tx");
                            let _ = self.db.execute_batch("RELEASE SAVEPOINT sqlitex_tx");
                        }
                    }
                }
            }

            let mut guard = RollbackGuard { db: db_ref, is_outermost, committed: false };

            let result = f(self);

            match result {
                Ok(val) => {
                    if is_outermost {
                        if let Err(e) = self.__db.execute_batch("COMMIT") {
                            return Err(sqlitex::errors::Error::from(e));
                        }
                    } else {
                        if let Err(e) = self.__db.execute_batch("RELEASE SAVEPOINT sqlitex_tx") {
                            return Err(sqlitex::errors::Error::from(e));
                        }
                    }
                    guard.committed = true;
                    Ok(val)
                }
                Err(e) => Err(e),
            }
        }

                //     pub fn transaction_immediate<T, F>(&mut self, f: F) -> Result<T, sqlitex::errors::Error>
                // where
                //     F: FnOnce(&mut Self) -> Result<T, sqlitex::errors::Error>,
                // {
                //     self.__db.execute_batch("BEGIN IMMEDIATE")
                //         .map_err(sqlitex::errors::Error::from)?;

                //     let result = f(self);

                //     match result {
                //         Ok(val) => {
                //             if let Err(e) = self.__db.execute_batch("COMMIT") {
                //                 return Err(sqlitex::errors::Error::from(e));
                //             }
                //             Ok(val)
                //         }
                //         Err(e) => {
                //             // Attempt rollback, ignoring failure since we are already erroring
                //             let _ = self.__db.execute_batch("ROLLBACK");
                //             Err(e)
                //         }
                //     }
                // }
                            #open_connected_db_method

                            #schema_init_method
                            #(#generated_methods)*
                        }
                    };
                },
        watcher_tokens,
    ))
}

fn parse_sql_macro_type(ty: &Type) -> syn::Result<Option<LitStr>> {
    if let Type::Macro(type_macro) = ty
        && type_macro.mac.path.is_ident("sql")
    {
        let lit = syn::parse2(type_macro.mac.tokens.clone()).map_err(|_| {
            syn::Error::new(
                type_macro.mac.tokens.span(),
                "sql!(...) must contain a string",
            )
        })?;

        return Ok(Some(lit));
    }

    Ok(None)
}

#[proc_macro_derive(SqlMapping)]
pub fn my_macro(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let struct_name = &input.ident;

    let name_as_string = struct_name.to_string();
    let new_name_string = format!("{}_", name_as_string);
    let mapper_struct_name = Ident::new(&new_name_string, struct_name.span());

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(fields_named) => &fields_named.named,
            _ => {
                return syn::Error::new(
                    input.ident.span(),
                    "SqlMapping only works on structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new(input.ident.span(), "SqlMapping only works on structs")
                .to_compile_error()
                .into();
        }
    };

    let field_bindings = fields.iter().enumerate().map(|(i, f)| {
        let field_name = f.ident.as_ref().unwrap();
        let field_type = &f.ty;
        let index = i as i32;

        quote! {
            let #field_name = unsafe
            {
            <#field_type as sqlitex::traits::from_sql::FromSql>::from_sql(stmt, #index)
            };

        }
    });

    let field_names = fields.iter().map(|f| f.ident.as_ref().unwrap());
    let expanded = quote! {
        #[derive(Clone, Debug)]
        pub struct #mapper_struct_name;

        impl sqlitex::traits::row_mapper::RowMapper for #mapper_struct_name {
            type Output = #struct_name;

            unsafe fn map_row(&self, stmt: *mut sqlitex::libsqlite3_sys::sqlite3_stmt) -> Self::Output {
                #(#field_bindings)*

                Self::Output {
                    #(#field_names),*
                }
            }
        }

        #[allow(non_upper_case_globals)]
        pub const #struct_name: #mapper_struct_name = #mapper_struct_name;
    };

    TokenStream::from(expanded)
}
