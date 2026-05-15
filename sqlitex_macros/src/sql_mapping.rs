use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident, parse_macro_input};

pub fn expand_sql_mapping(input: TokenStream) -> TokenStream {
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
            let #field_name = unsafe {
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
