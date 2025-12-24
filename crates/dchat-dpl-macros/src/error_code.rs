//! #[error_code] attribute macro implementation
//!
//! Generates error codes and human-readable messages for program errors.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse2, Data, DeriveInput, Fields, Result};

/// Implementation of the #[error_code] attribute macro.
pub fn error_code_impl(_attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
    let input: DeriveInput = parse2(item)?;
    let name = &input.ident;
    let vis = &input.vis;
    let attrs = &input.attrs;

    let data_enum = match &input.data {
        Data::Enum(e) => e,
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "#[error_code] can only be applied to enums",
            ))
        }
    };

    // Parse variants and their #[msg(...)] attributes
    let mut variant_defs = Vec::new();
    let mut code_arms = Vec::new();
    let mut msg_arms = Vec::new();
    let mut from_code_arms = Vec::new();

    // DPL errors start at 6000 to avoid conflicts with system errors
    const ERROR_CODE_OFFSET: u32 = 6000;

    for (i, variant) in data_enum.variants.iter().enumerate() {
        let variant_name = &variant.ident;
        // Use explicit #[code = N] if provided, otherwise auto-increment from 6000
        let code = get_code_attr(&variant.attrs).unwrap_or(ERROR_CODE_OFFSET + i as u32);

        // Get the error message from #[msg("...")] or #[msg = "..."] attribute
        let msg = get_msg_attr(&variant.attrs).unwrap_or_else(|| variant_name.to_string());

        match &variant.fields {
            Fields::Unit => {
                variant_defs.push(quote! { #variant_name });
            }
            Fields::Named(fields) => {
                let field_defs: Vec<_> = fields
                    .named
                    .iter()
                    .map(|f| {
                        let fname = &f.ident;
                        let fty = &f.ty;
                        quote! { #fname: #fty }
                    })
                    .collect();
                variant_defs.push(quote! { #variant_name { #(#field_defs),* } });
            }
            Fields::Unnamed(fields) => {
                let field_types: Vec<_> = fields.unnamed.iter().map(|f| &f.ty).collect();
                variant_defs.push(quote! { #variant_name(#(#field_types),*) });
            }
        }

        code_arms.push(quote! {
            #name::#variant_name { .. } => #code,
        });

        msg_arms.push(quote! {
            #name::#variant_name { .. } => #msg,
        });

        from_code_arms.push(quote! {
            #code => Some(stringify!(#variant_name)),
        });
    }

    // Generate the enum definition
    let enum_def = quote! {
        #(#attrs)*
        #[derive(Debug, Clone, PartialEq, Eq)]
        #vis enum #name {
            #(#variant_defs,)*
        }
    };

    // Generate error code methods
    let impl_block = quote! {
        impl #name {
            /// Returns the numeric error code.
            pub fn code(&self) -> u32 {
                match self {
                    #(#code_arms)*
                }
            }

            /// Returns the error message.
            pub fn msg(&self) -> &'static str {
                match self {
                    #(#msg_arms)*
                }
            }

            /// Attempts to get the variant name from an error code.
            pub fn from_code(code: u32) -> Option<&'static str> {
                match code {
                    #(#from_code_arms)*
                    _ => None,
                }
            }
        }
    };

    // Implement std::fmt::Display
    let display_impl = quote! {
        impl std::fmt::Display for #name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "Error {}: {}", self.code(), self.msg())
            }
        }
    };

    // Implement std::error::Error
    let error_impl = quote! {
        impl std::error::Error for #name {}
    };

    // Implement From for DplError
    let from_impl = quote! {
        impl From<#name> for dchat_dpl::DplError {
            fn from(e: #name) -> Self {
                dchat_dpl::DplError::Custom {
                    code: e.code(),
                    msg: e.msg().to_string(),
                }
            }
        }
    };

    // Implement Into<u64> for program return codes
    let into_u64_impl = quote! {
        impl From<#name> for u64 {
            fn from(e: #name) -> u64 {
                e.code() as u64
            }
        }
    };

    let output = quote! {
        #enum_def
        #impl_block
        #display_impl
        #error_impl
        #from_impl
        #into_u64_impl
    };

    Ok(output)
}

/// Extracts the message string from #[msg("...")] or #[msg = "..."] attribute.
fn get_msg_attr(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("msg") {
            // Try #[msg("...")] format first
            if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
                return Some(lit.value());
            }

            // Try #[msg = "..."] format
            if let syn::Meta::NameValue(meta) = &attr.meta {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit),
                    ..
                }) = &meta.value
                {
                    return Some(lit.value());
                }
            }
        }
    }
    None
}

/// Extracts the error code from #[code(N)] or #[code = N] attribute.
fn get_code_attr(attrs: &[syn::Attribute]) -> Option<u32> {
    for attr in attrs {
        if attr.path().is_ident("code") {
            // Try #[code(N)] format first
            if let Ok(lit) = attr.parse_args::<syn::LitInt>() {
                if let Ok(val) = lit.base10_parse::<u32>() {
                    return Some(val);
                }
            }

            // Try #[code = N] format
            if let syn::Meta::NameValue(meta) = &attr.meta {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(lit),
                    ..
                }) = &meta.value
                {
                    if let Ok(val) = lit.base10_parse::<u32>() {
                        return Some(val);
                    }
                }
            }
        }
    }
    None
}
