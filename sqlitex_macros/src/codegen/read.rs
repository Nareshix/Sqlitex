use proc_macro2::TokenStream;
use quote::quote;
use sqlitex_type_inference::{
    binding_patterns::BindingParam, expr::BaseType, table::ColumnInfo, QueryCardinality,
};
use crate::codegen::context::CodegenContext;

pub fn generate_read_methods(
    ctx: &CodegenContext,
    binding_types: &[BindingParam],
    param_names: &[String],
    select_types: &[ColumnInfo],
    cardinality: QueryCardinality,
) -> syn::Result<(TokenStream, TokenStream)> {
    let mut generated_structs = quote! {};
    let mut generated_methods = quote! {};

    let ident = ctx.ident;
    let field_attrs = ctx.field_attrs;
    let doc_comment = &ctx.doc_comment;
    let struct_name = ctx.struct_name;

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

    // PREFIXED to prevent naming collisions across identical methods in different tables
    let output_struct_name = quote::format_ident!("{}{}", struct_name, pascal_name);
    let mapper_struct_name = quote::format_ident!("{}{}_", struct_name, pascal_name);
    let scalar_mapper_name = quote::format_ident!("{}_{}_scalar_", struct_name, ident);

    let (method_args, bind_calls) = ctx.generate_bindings(binding_types, param_names)?;

    let single_col_rust_type = if is_single_col {
        let col = &select_types[0];
        let base_ty = match col.data_type.base_type {
            BaseType::Integer => quote! { i64 },
            BaseType::Real => quote! { f64 },
            BaseType::Text => quote! { String },
            BaseType::Blob => quote! { Vec<u8> },
            BaseType::Bool => quote! { bool },
            _ => quote! { i64 }, // fallback
        };
        if col.data_type.nullable {
            quote! { Option<#base_ty> }
        } else {
            quote! { #base_ty }
        }
    } else {
        quote! {}
    };

    if !is_single_col {
        let mut struct_fields = Vec::new();
        for col in select_types.iter() {
            let name = quote::format_ident!("{}", col.name);
            let base_ty = match col.data_type.base_type {
                BaseType::Integer => quote! { i64 },
                BaseType::Real => quote! { f64 },
                BaseType::Text => quote! { String },
                BaseType::Blob => quote! { Vec<u8> },
                BaseType::Bool => quote! { bool },
                _ => return Err(syn::Error::new(ctx.sql_span, "Unable to infer return type. Consider casting with `::` or `CAST AS`")),
            };
            let final_ty = if col.data_type.nullable {
                quote! { Option<#base_ty> }
            } else {
                quote! { #base_ty }
            };
            struct_fields.push(quote! { pub #name: #final_ty });
        }
        generated_structs.extend(quote! {
            #[derive(Clone, Debug, sqlitex::SqlMapping)]
            pub struct #output_struct_name {
                #(#struct_fields),*
            }
        });
    }

    if is_single_col {
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
            generated_structs.extend(quote! {
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
            generated_structs.extend(quote! {
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

    let prepare_block = ctx.generate_prepare_block();

    let ret_err_type = if binding_types.is_empty() {
        quote! { sqlitex::errors::SqlReadError }
    } else {
        quote! { sqlitex::errors::SqlReadErrorBindings }
    };

    match (cardinality, is_single_col) {
        (QueryCardinality::MaybeMany, false) => {
            generated_methods.extend(quote! {
                #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self #(, #method_args)*) -> Result<sqlitex::internal_sqlite::rows_dao::Rows<'_, #mapper_struct_name>, #ret_err_type> {
                    #prepare_block
                    #(#bind_calls)*
                    Ok(preparred_statement.query(#output_struct_name))
                }
            });
        }
        (QueryCardinality::MaybeMany, true) => {
            generated_methods.extend(quote! {
                #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self #(, #method_args)*) -> Result<sqlitex::internal_sqlite::rows_dao::Rows<'_, #scalar_mapper_name>, #ret_err_type> {
                    #prepare_block
                    #(#bind_calls)*
                    Ok(preparred_statement.query(#scalar_mapper_name))
                }
            });
        }
        (QueryCardinality::ZeroOrOne, false) => {
            generated_methods.extend(quote! {
                #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self #(, #method_args)*) -> Result<Option<#output_struct_name>, sqlitex::errors::Error> {
                    #prepare_block
                    #(#bind_calls)*
                    preparred_statement.query(#output_struct_name)
                        .first()
                        .map_err(sqlitex::errors::Error::from)
                }
            });
        }
        (QueryCardinality::ExactlyOne, false) => {
            generated_methods.extend(quote! {
                #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self #(, #method_args)*) -> Result<#output_struct_name, sqlitex::errors::Error> {
                    #prepare_block
                    #(#bind_calls)*
                    preparred_statement.query(#output_struct_name)
                        .first()
                        .map_err(sqlitex::errors::Error::from)
                        .and_then(|opt| opt.ok_or_else(|| {
                            sqlitex::errors::Error::Row(
                                sqlitex::errors::row::RowMapperError::SqliteFailure {
                                    code: sqlitex::libsqlite3_sys::SQLITE_ERROR,
                                    error_msg: "Aggregate query returned 0 rows unexpectedly".into()
                                }
                            )
                        }))
                }
            });
        }
        (QueryCardinality::ZeroOrOne, true) => {
            generated_methods.extend(quote! {
                #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self #(, #method_args)*) -> Result<Option<#single_col_rust_type>, sqlitex::errors::Error> {
                    #prepare_block
                    #(#bind_calls)*
                    preparred_statement.query(#scalar_mapper_name)
                        .first()
                        .map_err(sqlitex::errors::Error::from)
                }
            });
        }
        (QueryCardinality::ExactlyOne, true) => {
            generated_methods.extend(quote! {
                #(#field_attrs)*
                #[doc = #doc_comment]
                pub fn #ident(&mut self #(, #method_args)*) -> Result<#single_col_rust_type, sqlitex::errors::Error> {
                    #prepare_block
                    #(#bind_calls)*
                    preparred_statement.query(#scalar_mapper_name)
                        .first()
                        .map_err(sqlitex::errors::Error::from)
                        .and_then(|opt| opt.ok_or_else(|| {
                            sqlitex::errors::Error::Row(
                                sqlitex::errors::row::RowMapperError::SqliteFailure {
                                    code: sqlitex::libsqlite3_sys::SQLITE_ERROR,
                                    error_msg: "Aggregate query returned 0 rows unexpectedly".into()
                                }
                            )
                        }))
                }
            });
        }
    }

    Ok((generated_structs, generated_methods))
}