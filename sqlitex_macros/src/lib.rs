use std::{env, path::Path};

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemStruct, parse_macro_input};

mod codegen;
mod config;
mod migrations;
mod parse;
mod schema_source;
mod sql_mapping;
mod sqlite_validation;
mod utils;

use config::SqlitexConfig;

#[proc_macro_attribute]
pub fn sqlitex(args: TokenStream, input: TokenStream) -> TokenStream {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("No MANIFEST_DIR");
    let (config_opt, config_file_path) = SqlitexConfig::load_from_manifest(&manifest_dir);

    let path_lit_opt = if !args.is_empty() {
        match syn::parse::<syn::LitStr>(args) {
            Ok(lit) => {
                let full_path = Path::new(&manifest_dir).join(lit.value());
                Some(syn::LitStr::new(
                    full_path.to_str().expect("Invalid path"),
                    proc_macro2::Span::call_site(),
                ))
            }
            Err(_) => {
                return syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "sqlitex requires either no arguments or a path string to a sql/db file or folder.",
                )
                .to_compile_error()
                .into();
            }
        }
    } else if let Some(ref config) = config_opt {
        if let Some(ref schema) = config.schema {
            let full_path = Path::new(&manifest_dir).join(&schema.path);
            Some(syn::LitStr::new(
                full_path.to_str().expect("Invalid path"),
                proc_macro2::Span::call_site(),
            ))
        } else {
            None
        }
    } else {
        None
    };

    let mut item_struct = parse_macro_input!(input as ItemStruct);

    match codegen::expand(&mut item_struct, path_lit_opt.as_ref(), config_opt.as_ref()) {
        Ok((output, mut watcher)) => {
            // Watch sqlitex.toml for changes so recompilation is triggered
            if let Some(cfg_path) = config_file_path {
                let path_str = cfg_path.to_str().unwrap();
                watcher.extend(quote! {
                    const _: &[u8] = include_bytes!(#path_str);
                });
            }

            let final_output = quote! {
                #output
                #watcher
            };

            final_output.into()
        }
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro_derive(SqlMapping)]
pub fn my_macro(input: TokenStream) -> TokenStream {
    sql_mapping::expand_sql_mapping(input)
}