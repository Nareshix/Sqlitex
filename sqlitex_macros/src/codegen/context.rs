use proc_macro2::{Span, TokenStream};
use quote::quote;
use sqlitex_type_inference::{binding_patterns::BindingParam, expr::BaseType};
use syn::{Attribute, Ident};

pub struct CodegenContext<'a> {
    pub struct_name: &'a Ident,
    pub ident: &'a Ident,
    pub field_attrs: &'a [Attribute],
    pub doc_comment: String,
    pub sql_span: Span,
}

impl<'a> CodegenContext<'a> {
    pub fn generate_prepare_block(&self) -> TokenStream {
        let ident = self.ident;
        quote! {
            let mut __stmt = std::ptr::null_mut();
            unsafe {
                sqlitex::utility::utils::prepare_stmt(
                    self.__db.db,
                    &mut __stmt,
                    self.#ident.sql_query
                )?;
            }
            let mut preparred_statement = sqlitex::internal_sqlite::preparred_statement::PreparredStmt {
                stmt: __stmt,
                conn: self.__db.db,
            };
        }
    }
    /// Automatically infers the Rust types based on BaseType, constructs the function parameters
    /// (e.g. `arg_1: String`) and returns the bind calls. Both read and write logic share this uniformly!
    pub fn generate_bindings(
        &self,
        binding_types: &[BindingParam],
        param_names: &[String],
    ) -> syn::Result<(Vec<TokenStream>, Vec<TokenStream>)> {
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
                        self.sql_span,
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

        Ok((method_args, bind_calls))
    }
}
