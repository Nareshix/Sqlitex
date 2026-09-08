use quote::quote;
use std::collections::HashMap;
use syn::{ItemStruct, parse_quote, spanned::Spanned};

use sqlitex_type_inference::{
    binding_patterns::get_type_of_binding_parameters, detect_query_cardinality, is_create_table,
    pg_cast_syntax_to_sqlite, rewrite_bool_columns, select_patterns::get_types_from_select,
    table::create_tables, validate_cast_types, validate_create_table_types, validate_insert_strict,
    validate_no_virtual_tables, validate_single_statement,
};

use crate::sqlite_validation::validate_sql_syntax_with_sqlite;
use crate::utils::*;
use crate::{migrations, parse::*, schema_source};

pub mod context;
pub mod create_table;
pub mod read;
pub mod runtime;
pub mod write;

use context::CodegenContext;

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
        let ident = field.ident.clone().unwrap();
        let field_attrs = field.attrs.clone();

        validate_field_name(&ident, db_path_lit)?;

        let sql_macro_opt = parse_sql_macro_type(&field.ty)?;
        let runtime_macro_opt = parse_runtime_macro(&field.ty)?;

        // If the field is EITHER `sql!` OR `sql_escape_hatch!`
        if sql_macro_opt.is_some() || runtime_macro_opt.is_some() {
            let sql_lit = if let Some(lit) = &sql_macro_opt {
                lit
            } else {
                &runtime_macro_opt.as_ref().unwrap().sql
            };

            let sql_query = prepare_sql_string(&sql_lit.value(), sql_lit.span())?;
            field.ty = parse_quote!(sqlitex::internal_sqlite::sqlitex_statement::SqlitexStmt);
            push_sql_assignment(&ident, &sql_query, sql_lit.span(), &mut sql_assignments);

            let ctx = CodegenContext {
                struct_name,
                ident: &ident,
                field_attrs: &field_attrs,
                doc_comment: format!(" \n**SQL**\n```sql\n{}", format_sql(&sql_query)),
                sql_span: sql_lit.span(),
            };

            if let Some(runtime_input) = runtime_macro_opt {
                //  sql_escape_hatch!
                generated_methods.push(runtime::generate_runtime_method(&ctx, &runtime_input));
            } else {
                //  sql!
                validate_no_virtual_tables(&sql_query)
                    .map_err(|msg| syn::Error::new(sql_lit.span(), msg))?;
                validate_cast_types(&sql_query)
                    .map_err(|msg| syn::Error::new(sql_lit.span(), msg))?;
                validate_create_table_types(&sql_query)
                    .map_err(|msg| syn::Error::new(sql_lit.span(), msg))?;

                if let Err(err_msg) = validate_single_statement(&sql_query) {
                    return Err(syn::Error::new(sql_lit.span(), err_msg));
                }
                if let Err(err_msg) = validate_sql_syntax_with_sqlite(&all_tables, &sql_query) {
                    return Err(syn::Error::new(sql_lit.span(), err_msg.to_string()));
                }
                if let Err(err_msg) = validate_insert_strict(&sql_query, &all_tables) {
                    return Err(syn::Error::new(sql_lit.span(), err_msg.to_string()));
                }

                if is_create_table(&sql_query) {
                    create_tables(&sql_query, &mut all_tables);
                    generated_methods.push(create_table::generate_create_table(&ctx));
                    continue;
                }

                let select_types =
                    get_types_from_select(&sql_query, &all_tables).map_err(|err_msg| {
                        syn::Error::new(sql_lit.span(), format!("Return Type Error: {}", err_msg))
                    })?;

                let binding_types = get_type_of_binding_parameters(&sql_query, &all_tables)
                    .map_err(|err| format_binding_error(err, &sql_query, sql_lit.span()))?;

                let param_names = generate_unique_param_names(&binding_types);

                if !select_types.is_empty() {
                    let cardinality = detect_query_cardinality(&sql_query, &all_tables);
                    let (structs, methods) = read::generate_read_methods(
                        &ctx,
                        &binding_types,
                        &param_names,
                        &select_types,
                        cardinality,
                    )?;
                    generated_structs.push(structs);
                    generated_methods.push(methods);
                } else {
                    generated_methods.push(write::generate_write_methods(
                        &ctx,
                        &binding_types,
                        &param_names,
                    )?);
                }
            }
        } else {
            let ty = &field.ty;
            standard_params.push(quote! { #ident: #ty });
            standard_assignments.push(quote! { #ident });
        }
    }

    fields.named.push(parse_quote! { __db: std::sync::Arc<sqlitex::internal_sqlite::sqlitex_connection::Connection> });

    let (impl_generics, ty_generics, where_clause) = item_struct.generics.split_for_impl();
    item_struct.vis = parse_quote!(pub);

    let transaction_method = generate_transaction_method();

    Ok((
        quote! {
            #(#generated_structs)*
            #item_struct

            const _: () = {
                impl #impl_generics #struct_name #ty_generics #where_clause {
                    pub fn new(
                        __db_conn: impl Into<std::sync::Arc<sqlitex::internal_sqlite::sqlitex_connection::Connection>>,
                        #(#standard_params),*
                    ) -> Self {
                        Self {
                            __db: __db_conn.into(),
                            #(#standard_assignments,)*
                            #(#sql_assignments,)*
                        }
                    }

                    #transaction_method
                    #open_connected_db_method
                    #schema_init_method
                    #(#generated_methods)*
                }
            };
        },
        watcher_tokens,
    ))
}

fn validate_field_name(ident: &syn::Ident, db_path_lit: Option<&syn::LitStr>) -> syn::Result<()> {
    if ident == "transaction" {
        return Err(syn::Error::new(
            ident.span(),
            "`transaction` is a reserved keyword. Rename this field to something else.",
        ));
    }
    if ident == "init" && db_path_lit.is_some() && db_path_lit.unwrap().value().ends_with(".sql") {
        return Err(syn::Error::new(
            ident.span(),
            "`init` is a reserved keyword when pointing to an external .sql file. Rename this field to something else.",
        ));
    }
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
    Ok(())
}

fn prepare_sql_string(raw_sql: &str, span: proc_macro2::Span) -> syn::Result<String> {
    let sql = pg_cast_syntax_to_sqlite(raw_sql);
    rewrite_bool_columns(&sql).map_err(|msg| syn::Error::new(span, msg))
}

fn push_sql_assignment(
    ident: &syn::Ident,
    sql_query: &str,
    span: proc_macro2::Span,
    sql_assignments: &mut Vec<proc_macro2::TokenStream>,
) {
    let transpiled_sql_lit = syn::LitStr::new(sql_query, span);
    sql_assignments.push(quote! {
        #ident: sqlitex::internal_sqlite::sqlitex_statement::SqlitexStmt {
            sql_query: #transpiled_sql_lit,
        }
    });
}
fn generate_unique_param_names(
    binding_types: &[sqlitex_type_inference::binding_patterns::BindingParam],
) -> Vec<String> {
    let mut param_names = Vec::new();
    let mut used_names = std::collections::HashSet::new();

    for (i, param) in binding_types.iter().enumerate() {
        let mut base_name = param.name.clone();
        if base_name == "arg" || base_name.is_empty() {
            base_name = format!("arg_{}", i);
        }
        base_name = base_name.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        if base_name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            base_name = format!("arg_{}", base_name);
        }
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
        let mut final_name = base_name.clone();
        let mut counter = 1;
        while used_names.contains(&final_name) {
            final_name = format!("{}_{}", base_name, counter);
            counter += 1;
        }
        used_names.insert(final_name.clone());
        param_names.push(final_name);
    }
    param_names
}

fn format_binding_error(
    err: sqlitex_type_inference::binding_patterns::InferenceError,
    sql_query: &str,
    span: proc_macro2::Span,
) -> syn::Error {
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
    syn::Error::new(span, msg)
}

fn generate_transaction_method() -> proc_macro2::TokenStream {
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

    quote! {
        #[doc = #transaction_doc]
        pub fn transaction<T, F>(&self, f: F) -> Result<T, sqlitex::errors::Error>
        where
            F: FnOnce(&Self) -> Result<T, sqlitex::errors::Error>,
        {
            let is_outermost = unsafe {
                sqlitex::libsqlite3_sys::sqlite3_get_autocommit(self.__db.db) != 0
            };

            if is_outermost {
                self.__db.execute_batch("BEGIN IMMEDIATE").map_err(sqlitex::errors::Error::from)?;
            } else {
                self.__db.execute_batch("SAVEPOINT sqlitex_tx").map_err(sqlitex::errors::Error::from)?;
            }

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
    }
}
