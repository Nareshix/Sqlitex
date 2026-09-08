use proc_macro2::TokenStream;
use quote::quote;
use sqlitex_type_inference::{binding_patterns::BindingParam, expr::BaseType};
use crate::codegen::context::CodegenContext;

pub fn generate_write_methods(
    ctx: &CodegenContext,
    binding_types: &[BindingParam],
    param_names: &[String],
) -> syn::Result<TokenStream> {
    let mut generated_methods = quote! {};
    let ident = ctx.ident;
    let field_attrs = ctx.field_attrs;
    let doc_comment = &ctx.doc_comment;

    let prepare_block = ctx.generate_prepare_block();

    if binding_types.is_empty() {
        generated_methods.extend(quote! {
            #(#field_attrs)*
            #[doc = #doc_comment]
            pub fn #ident(&self) -> Result<(), sqlitex::errors::SqlWriteError> {
                #prepare_block
                preparred_statement.step()?;
                Ok(())
            }
        });
    } else {
        // Call unified Bindings Logic
        let (method_args, bind_calls) = ctx.generate_bindings(binding_types, param_names)?;

        generated_methods.extend(quote! {
            #(#field_attrs)*
            #[doc = #doc_comment]
            pub fn #ident(&self #(, #method_args)*) -> Result<(), sqlitex::errors::SqlWriteBindingError> {
                #prepare_block
                #(#bind_calls)*
                preparred_statement.step()?;
                Ok(())
            }
        });

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
                _ => return Err(syn::Error::new(ctx.sql_span, "Unable to infer type for `?`. Consider casting with `::` or `CAST AS`")),
            };

            let owned_final_type = if bind_type.nullable {
                quote! { Option<#owned_base_type> }
            } else {
                quote! { #owned_base_type }
            };

            many_owned_types.push(owned_final_type);

            let bind_expr = if bind_type.nullable {
                match bind_type.base_type {
                    BaseType::Text | BaseType::Blob => quote! { item.#tuple_idx.as_deref() },
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
                    BaseType::Text | BaseType::Blob => quote! { item.as_deref() },
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

        generated_methods.extend(quote! {
            #(#field_attrs)*
            #[doc = #many_doc_header]
            #[doc = #doc_comment]
pub fn #many_ident(&self, items: &[#item_type]) -> Result<(), sqlitex::errors::Error> {
                if items.is_empty() {
                    return Ok(());
                }

                let mut __bulk_stmt = std::ptr::null_mut();
                unsafe {
                    sqlitex::utility::utils::prepare_stmt(
                        self.__db.db,
                        &mut __bulk_stmt,
                        self.#ident.sql_query
                    ).map_err(|e| sqlitex::errors::Error::from(sqlitex::errors::SqlWriteBindingError::Prepare(e)))?;
                }

                let mut preparred_statement = sqlitex::internal_sqlite::preparred_statement::PreparredStmt {
                    stmt: __bulk_stmt,
                    conn: self.__db.db,
                };

                let is_outermost = unsafe { sqlitex::libsqlite3_sys::sqlite3_get_autocommit(self.__db.db) != 0 };

                if is_outermost {
                    self.__db.execute_batch("BEGIN IMMEDIATE").map_err(sqlitex::errors::Error::from)?;
                } else {
                    self.__db.execute_batch("SAVEPOINT sqlitex_batch").map_err(sqlitex::errors::Error::from)?;
                }

                for item in items {
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

                    unsafe {
                        sqlitex::libsqlite3_sys::sqlite3_reset(preparred_statement.stmt);
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
    }
    Ok(generated_methods)
}