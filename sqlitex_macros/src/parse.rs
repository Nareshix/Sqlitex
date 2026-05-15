use syn::{LitStr, Type, spanned::Spanned};

pub(crate) struct RuntimeSqlInput {
    pub return_type: Option<Type>,
    pub sql: syn::LitStr,
    pub args: Vec<Type>,
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
pub(crate) fn parse_runtime_macro(ty: &syn::Type) -> syn::Result<Option<RuntimeSqlInput>> {
    if let syn::Type::Macro(type_macro) = ty
        && type_macro.mac.path.is_ident("sql_escape_hatch")
    {
        let parsed: RuntimeSqlInput = syn::parse2(type_macro.mac.tokens.clone())?;
        return Ok(Some(parsed));
    }
    Ok(None)
}
pub(crate) fn parse_sql_macro_type(ty: &Type) -> syn::Result<Option<LitStr>> {
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
