use sqlformat::{FormatOptions, Indent, QueryParams, format};
use std::{collections::HashMap, env, path::Path};

use proc_macro::TokenStream;
use quote::quote;
use sqlitex_core::utility::utils::{get_db_schema, validate_sql_syntax_with_sqlite};
use sqlitex_type_inference::{
    binding_patterns::get_type_of_binding_parameters, expr::BaseType, is_create_table,
    pg_cast_syntax_to_sqlite, rewrite_bool_columns, select_patterns::get_types_from_select,
    table::create_tables, validate_cast_types, validate_create_table_types, validate_insert_strict,
    validate_no_virtual_tables, validate_single_statement,
};
use syn::{
    Data, DeriveInput, Fields, Ident, ItemStruct, LitStr, Type, parse_macro_input, parse_quote,
    spanned::Spanned,
};

/// This nicely formats the sql string.
///
/// Useful for vscode hover over fn
fn format_sql(sql: &str) -> String {
    let options = FormatOptions {
        indent: Indent::Tabs,
        ..Default::default()
    };
    format(sql, &QueryParams::None, &options)
}

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
        Ok(output) => {
            let watcher = if let Some(abs_path) = path_lit_opt {
                quote! {
                    const _: &[u8] = include_bytes!(#abs_path);
                }
            } else {
                quote! {}
            };

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
) -> syn::Result<proc_macro2::TokenStream> {
    let mut all_tables = HashMap::new();

    let mut schema_init_method = quote! {};

    if let Some(path) = db_path_lit {
        let db_path = path.value();

        if db_path.ends_with(".sql") {
            let content = std::fs::read_to_string(&db_path).map_err(|e| {
                syn::Error::new(path.span(), format!("Failed to read {}: {}", db_path, e))
            })?;

            validate_cast_types(&content)
                .map_err(|msg| syn::Error::new(path.span(), format!("In {}: {}", db_path, msg)))?;

            validate_create_table_types(&content)
                .map_err(|msg| syn::Error::new(path.span(), format!("In {}: {}", db_path, msg)))?;

            sqlitex_type_inference::validate_sql_file_syntax(&content)
                .map_err(|msg| syn::Error::new(path.span(), format!("In {}: {}", db_path, msg)))?;

            // Generate the init method only if a .sql file is provided
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

        let schemas = get_db_schema(&db_path).map_err(|err| {
            syn::Error::new(path.span(), format!("Failed to load DB schema: {}", err))
        })?;
        for schema in schemas {
            validate_create_table_types(&schema)
                .map_err(|msg| syn::Error::new(path.span(), format!("In {}: {}", db_path, msg)))?;
            create_tables(&schema, &mut all_tables);
        }
    }

    let mut open_connected_db_method = quote! {};

    if let Some(path) = db_path_lit {
        let db_path = path.value();
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

        // if ident == "transaction_immediate" {
        //     return Err(syn::Error::new(
        //         ident.span(),
        //         "`transaction_immediate` is a reserved keyword. Rename this field to something else.",
        //     ));
        // }

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

                for (i, bind_type) in binding_types.iter().enumerate() {
                    let arg_name = quote::format_ident!("arg_{}", i);
                    let bind_index = (i + 1) as i32;

                    let rust_base_type = match bind_type.base_type {
                        BaseType::Integer => quote! { i64 },
                        BaseType::Real => quote! { f64 },
                        BaseType::Bool => quote! { bool },
                        BaseType::Text => quote! { &str },
                        BaseType::Blob => quote! { &[u8] },
                        _ => quote! {},
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

                // generate _many method
                let many_ident = quote::format_ident!("{}_many", ident);

                let mut many_owned_types = Vec::new();
                let mut many_bind_calls = Vec::new();

                for (i, bind_type) in binding_types.iter().enumerate() {
                    let bind_index = (i + 1) as i32;
                    let tuple_idx = syn::Index::from(i);

                    let owned_base_type = match bind_type.base_type {
                        BaseType::Integer => quote! { i64 },
                        BaseType::Real => quote! { f64 },
                        BaseType::Bool => quote! { bool },
                        BaseType::Text => quote! { String },
                        BaseType::Blob => quote! { Vec<u8> },
                        _ => quote! {},
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

                let (item_type, final_many_bind_calls) = if binding_types.len() == 1 {
                    let bind_type = &binding_types[0];
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

db.{}_many(&bulk)?;
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

                            #(#final_many_bind_calls)*

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

                let struct_name = quote::format_ident!("{}", pascal_name);
                let mapper_struct_name = quote::format_ident!("{}_", pascal_name);

                re_exports.push(struct_name.clone());

                let mut struct_fields = Vec::new();

                for col in select_types.iter() {
                    let name = quote::format_ident!("{}", col.name);

                    let base_ty = match col.data_type.base_type {
                        BaseType::Integer => quote! { i64 },
                        BaseType::Real => quote! { f64 },
                        BaseType::Text => quote! { String },
                        BaseType::Blob => quote! { Vec<u8> },
                        BaseType::Bool => quote! { bool },
                        _ => quote! {},
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
                    pub struct #struct_name {
                        #(#struct_fields),*
                    }
                });

                generated_methods.push(quote! {
                    #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self) -> Result<sqlitex::internal_sqlite::rows_dao::Rows<'_, #mapper_struct_name>, sqlitex::errors::SqlReadError> {
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
            Ok(preparred_statement.query(#struct_name))
        }
    });
            } else {
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
                        _ => quote! {},
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

                let mut method_args = Vec::new();
                let mut bind_calls = Vec::new();

                for (i, bind_type) in binding_types.iter().enumerate() {
                    let arg_name = quote::format_ident!("arg_{}", i);
                    let bind_index = (i + 1) as i32;

                    let rust_base_type = match bind_type.base_type {
                        BaseType::Integer => quote! { i64 },
                        BaseType::Real => quote! { f64 },
                        BaseType::Bool => quote! { bool },
                        BaseType::Text => quote! { &str },
                        BaseType::Blob => quote! { &[u8] },
                        _ => quote! {},
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
                    pub fn #ident(&mut self, #(#method_args),*) -> Result<sqlitex::internal_sqlite::rows_dao::Rows<'_, #mapper_struct_name>, sqlitex::errors::SqlReadErrorBindings> {
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

                        Ok(preparred_statement.query(#output_struct_name))
                    }
                });
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

    let mod_name =
        quote::format_ident!("__sqlitex_inner_{}", struct_name.to_string().to_lowercase());

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

    Ok(quote! {
            #[doc(hidden)]
            mod #mod_name {
                use super::*;
                #(#generated_structs)*
                #item_struct

                impl #impl_generics #struct_name #ty_generics #where_clause {
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
                Ok(val)
            }
            Err(e) => {
                if is_outermost {
                    let _ = self.__db.execute_batch("ROLLBACK");
                } else {
                    // Rollback the savepoint, then release it to pop it off the stack
                    let _ = self.__db.execute_batch("ROLLBACK TO SAVEPOINT sqlitex_tx");
                    let _ = self.__db.execute_batch("RELEASE SAVEPOINT sqlitex_tx");
                }
                Err(e)
            }
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
            }

            pub use #mod_name::#struct_name;
        })
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

    // TODO error handling
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(fields_named) => &fields_named.named,
            _ => panic!("This macro only works on structs with named fields"),
        },
        _ => panic!("This macro only works on structs"),
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
