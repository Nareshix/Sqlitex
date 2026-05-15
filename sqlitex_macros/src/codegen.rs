use std::collections::HashMap;

use crate::sqlite_validation::validate_sql_syntax_with_sqlite;
use quote::quote;
use sqlitex_type_inference::validate_create_table_types;
use sqlitex_type_inference::{
    QueryCardinality, binding_patterns::get_type_of_binding_parameters, detect_query_cardinality,
    expr::BaseType, is_create_table, pg_cast_syntax_to_sqlite, rewrite_bool_columns,
    select_patterns::get_types_from_select, table::create_tables, validate_cast_types,
    validate_insert_strict, validate_no_virtual_tables, validate_single_statement,
};
use syn::{ItemStruct, parse_quote, spanned::Spanned};

use crate::utils::*;
use crate::{migrations, parse::*, schema_source};

pub(crate) fn expand(
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
            let result = migrations::process_migrations_dir(path, &db_path)?;
            schema_init_method = result.schema_init_method;
            watcher_tokens = result.watcher_tokens;
            all_tables = result.all_tables;
        } else {
            let result = schema_source::process_file_source(path, &db_path)?;
            schema_init_method = result.schema_init_method;
            open_connected_db_method = result.open_connected_db_method;
            watcher_tokens = result.watcher_tokens;
            all_tables = result.all_tables;
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

                let output_struct_name = quote::format_ident!("{}{}", struct_name, pascal_name);
                let mapper_struct_name = quote::format_ident!("{}{}_", struct_name, pascal_name);
                let scalar_mapper_name = quote::format_ident!("{}_{}_scalar_", struct_name, ident);

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
