use std::{env, path::Path};

use proc_macro::TokenStream;
use quote::quote;
use syn::{ItemStruct, parse_macro_input};
mod codegen;
mod migrations;
mod parse;
mod schema_source;
mod sql_mapping;
mod utils;

use codegen::expand;
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

#[proc_macro_derive(SqlMapping)]
pub fn my_macro(input: TokenStream) -> TokenStream {
    sql_mapping::expand_sql_mapping(input)
}
