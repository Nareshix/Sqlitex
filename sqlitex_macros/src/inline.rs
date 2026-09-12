use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Expr, LitStr, Token, Type,
    parse::{Parse, ParseStream},
};

use sqlitex_type_inference::{
    QueryCardinality, binding_patterns::get_type_of_binding_parameters, detect_query_cardinality,
    expr::BaseType, pg_cast_syntax_to_sqlite, rewrite_bool_columns,
    select_patterns::get_types_from_select, validate_cast_types, validate_create_table_types,
    validate_insert_strict, validate_no_virtual_tables, validate_single_statement,
};

use crate::{
    config::SqlitexConfig, migrations, schema_source,
    sqlite_validation::validate_sql_syntax_with_sqlite,
};

pub struct QueryInput {
    pub target_type: Option<Type>,
    pub sql: LitStr,
    pub args: Vec<Expr>,
}

impl Parse for QueryInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // If query_as!(TargetType, "SQL", ...)
        let (target_type, sql) = if input.peek(LitStr) {
            let sql: LitStr = input.parse()?;
            (None, sql)
        } else {
            let target: Type = input.parse()?;
            input.parse::<Token![,]>()?;
            let sql: LitStr = input.parse()?;
            (Some(target), sql)
        };

        let mut args = Vec::new();
        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            args.push(input.parse()?);
        }

        Ok(QueryInput {
            target_type,
            sql,
            args,
        })
    }
}

pub fn expand_query(input: QueryInput) -> syn::Result<TokenStream> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("No MANIFEST_DIR");
    let (config_opt, config_file_path) = SqlitexConfig::load_from_manifest(&manifest_dir);

    let config = config_opt.ok_or_else(|| {
        syn::Error::new(
            input.sql.span(),
            "sqlitex.toml not found in project root. Create sqlitex.toml with [schema] path = '...'",
        )
    })?;

    let schema_cfg = config.schema.ok_or_else(|| {
        syn::Error::new(
            input.sql.span(),
            "[schema] section with `path` must be defined in sqlitex.toml",
        )
    })?;

    let full_schema_path = std::path::Path::new(&manifest_dir).join(&schema_cfg.path);
    let full_schema_str = full_schema_path.to_str().expect("Invalid schema path");
    let schema_path_lit = syn::LitStr::new(full_schema_str, input.sql.span());

    let (all_tables, mut watcher_tokens) =
        if full_schema_path.is_dir() || schema_cfg.path.ends_with('/') {
            let result = migrations::process_migrations_dir(&schema_path_lit, full_schema_str)?;
            (result.all_tables, result.watcher_tokens)
        } else {
            let result = schema_source::process_file_source(&schema_path_lit, full_schema_str)?;
            (result.all_tables, result.watcher_tokens)
        };

    if let Some(cfg_path) = config_file_path {
        let cfg_path_str = cfg_path.to_str().unwrap();
        watcher_tokens.extend(quote! {
            const _: &[u8] = include_bytes!(#cfg_path_str);
        });
    }

    let raw_sql = input.sql.value();
    let sql_query = pg_cast_syntax_to_sqlite(&raw_sql);
    let sql_query =
        rewrite_bool_columns(&sql_query).map_err(|e| syn::Error::new(input.sql.span(), e))?;

    validate_no_virtual_tables(&sql_query).map_err(|e| syn::Error::new(input.sql.span(), e))?;
    validate_cast_types(&sql_query).map_err(|e| syn::Error::new(input.sql.span(), e))?;
    validate_create_table_types(&sql_query).map_err(|e| syn::Error::new(input.sql.span(), e))?;
    validate_single_statement(&sql_query).map_err(|e| syn::Error::new(input.sql.span(), e))?;
    validate_sql_syntax_with_sqlite(&all_tables, &sql_query)
        .map_err(|e| syn::Error::new(input.sql.span(), e))?;
    validate_insert_strict(&sql_query, &all_tables)
        .map_err(|e| syn::Error::new(input.sql.span(), e))?;

    let expected_bindings = get_type_of_binding_parameters(&sql_query, &all_tables)
        .map_err(|err| syn::Error::new(input.sql.span(), err.message))?;

    if input.args.len() != expected_bindings.len() {
        return Err(syn::Error::new(
            input.sql.span(),
            format!(
                "Argument count mismatch: Query expects {} parameter(s) (?), but {} argument(s) were provided.",
                expected_bindings.len(),
                input.args.len()
            ),
        ));
    }

    let mut bind_calls = Vec::new();
    for (i, arg_expr) in input.args.iter().enumerate() {
        let idx = (i + 1) as i32;
        bind_calls.push(quote! {
            __stmt.bind_parameter(#idx, #arg_expr)?;
        });
    }

    let binder = quote! {
        move |__stmt: &sqlitex::__private::preparred_statement::PreparredStmt| -> Result<(), sqlitex::__private::SqliteFailure> {
            #(#bind_calls)*
            Ok(())
        }
    };

    let select_types = get_types_from_select(&sql_query, &all_tables)
        .map_err(|err| syn::Error::new(input.sql.span(), err))?;

    let transpiled_sql_lit = syn::LitStr::new(&sql_query, input.sql.span());

    // 1. WRITE QUERY (INSERT, UPDATE, DELETE, CREATE)
    if select_types.is_empty() {
        return Ok(quote! {
            {
                #watcher_tokens
                sqlitex::__private::WriteQuery {
                    sql: #transpiled_sql_lit,
                    binder: #binder,
                }
            }
        });
    }

    // 2. READ QUERY (SELECT)
    let cardinality = detect_query_cardinality(&sql_query, &all_tables);

    let (_output_type, mapper_struct) = if let Some(target) = input.target_type {
        let mapper_impl = if let Type::Tuple(tuple_type) = &target {
            if tuple_type.elems.len() != select_types.len() {
                return Err(syn::Error::new_spanned(
                    &target,
                    format!(
                        "Tuple element count mismatch: target tuple has {} element(s), but query returns {} column(s).",
                        tuple_type.elems.len(),
                        select_types.len()
                    ),
                ));
            }

            let mut tuple_reads = Vec::new();
            for (i, col) in select_types.iter().enumerate() {
                let idx = i as i32;
                let base_ty = match col.data_type.base_type {
                    BaseType::Integer => quote! { i64 },
                    BaseType::Real => quote! { f64 },
                    BaseType::Text => quote! { String },
                    BaseType::Blob => quote! { Vec<u8> },
                    BaseType::Bool => quote! { bool },
                    _ => quote! { i64 },
                };
                let col_ty = if col.data_type.nullable {
                    quote! { Option<#base_ty> }
                } else {
                    quote! { #base_ty }
                };

                tuple_reads.push(quote! {
                    <#col_ty as sqlitex::__private::from_sql::FromSql>::from_sql(stmt, #idx)
                });
            }

            let tuple_constructor = if tuple_reads.len() == 1 {
                quote! { ( #(#tuple_reads),* , ) }
            } else {
                quote! { ( #(#tuple_reads),* ) }
            };

            quote! {
                pub struct __Mapper;
                impl sqlitex::__private::row_mapper::RowMapper for __Mapper {
                    type Output = #target;
                    unsafe fn map_row(&self, stmt: *mut sqlitex::__private::libsqlite3_sys::sqlite3_stmt) -> Self::Output {
                        #tuple_constructor
                    }
                }
            }
        } else if select_types.len() == 1 {
            let col = &select_types[0];
            let base_ty = match col.data_type.base_type {
                BaseType::Integer => quote! { i64 },
                BaseType::Real => quote! { f64 },
                BaseType::Text => quote! { String },
                BaseType::Blob => quote! { Vec<u8> },
                BaseType::Bool => quote! { bool },
                _ => quote! { i64 },
            };
            let col_ty = if col.data_type.nullable {
                quote! { Option<#base_ty> }
            } else {
                quote! { #base_ty }
            };

            quote! {
                pub struct __Mapper;
                impl sqlitex::__private::row_mapper::RowMapper for __Mapper {
                    type Output = #target;
                    unsafe fn map_row(&self, stmt: *mut sqlitex::__private::libsqlite3_sys::sqlite3_stmt) -> Self::Output {
                        <#col_ty as sqlitex::__private::from_sql::FromSql>::from_sql(stmt, 0)
                    }
                }
            }
        } else {
            let mut field_reads = Vec::new();
            for (i, col) in select_types.iter().enumerate() {
                let idx = i as i32;
                let ident = quote::format_ident!("{}", col.name);
                let base_ty = match col.data_type.base_type {
                    BaseType::Integer => quote! { i64 },
                    BaseType::Real => quote! { f64 },
                    BaseType::Text => quote! { String },
                    BaseType::Blob => quote! { Vec<u8> },
                    BaseType::Bool => quote! { bool },
                    _ => quote! { i64 },
                };
                let col_ty = if col.data_type.nullable {
                    quote! { Option<#base_ty> }
                } else {
                    quote! { #base_ty }
                };

                field_reads.push(quote! {
                    #ident: <#col_ty as sqlitex::__private::from_sql::FromSql>::from_sql(stmt, #idx)
                });
            }

            quote! {
                pub struct __Mapper;
                impl sqlitex::__private::row_mapper::RowMapper for __Mapper {
                    type Output = #target;
                    unsafe fn map_row(&self, stmt: *mut sqlitex::__private::libsqlite3_sys::sqlite3_stmt) -> Self::Output {
                        #target {
                            #(#field_reads),*
                        }
                    }
                }
            }
        };

        (quote! { #target }, mapper_impl)
    } else {
        let mut struct_fields = Vec::new();
        let mut field_reads = Vec::new();

        for (i, col) in select_types.iter().enumerate() {
            let idx = i as i32;
            let ident = quote::format_ident!("{}", col.name);
            let base_ty = match col.data_type.base_type {
                BaseType::Integer => quote! { i64 },
                BaseType::Real => quote! { f64 },
                BaseType::Text => quote! { String },
                BaseType::Blob => quote! { Vec<u8> },
                BaseType::Bool => quote! { bool },
                _ => quote! { i64 },
            };
            let col_ty = if col.data_type.nullable {
                quote! { Option<#base_ty> }
            } else {
                quote! { #base_ty }
            };

            struct_fields.push(quote! { pub #ident: #col_ty });
            field_reads.push(quote! {
                #ident: <#col_ty as sqlitex::__private::from_sql::FromSql>::from_sql(stmt, #idx)
            });
        }

        let mapper_impl = quote! {
            #[derive(Clone, Debug, sqlitex::__private::serde::Serialize, sqlitex::__private::serde::Deserialize)]
            #[serde(crate = "sqlitex::__private::serde")]
            pub struct __Record {
                #(#struct_fields),*
            }

            pub struct __Mapper;
            impl sqlitex::__private::row_mapper::RowMapper for __Mapper {
                type Output = __Record;
                unsafe fn map_row(&self, stmt: *mut sqlitex::__private::libsqlite3_sys::sqlite3_stmt) -> Self::Output {
                    __Record {
                        #(#field_reads),*
                    }
                }
            }
        };

        (quote! { __Record }, mapper_impl)
    };

    let query_runner = match cardinality {
        QueryCardinality::ExactlyOne => quote! {
            sqlitex::__private::ScalarQuery {
                sql: #transpiled_sql_lit,
                mapper: __Mapper,
                binder: #binder,
            }
        },
        QueryCardinality::ZeroOrOne => quote! {
            sqlitex::__private::OptionalQuery {
                sql: #transpiled_sql_lit,
                mapper: __Mapper,
                binder: #binder,
            }
        },
        QueryCardinality::MaybeMany => quote! {
            sqlitex::__private::ManyQuery {
                sql: #transpiled_sql_lit,
                mapper: __Mapper,
                binder: #binder,
            }
        },
    };

    Ok(quote! {
        {
            #watcher_tokens
            #mapper_struct
            #query_runner
        }
    })
}