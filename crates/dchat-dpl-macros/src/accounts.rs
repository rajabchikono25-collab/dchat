//! #[derive(Accounts)] macro implementation
//!
//! Generates account validation and deserialization for accounts structs.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse::Parse, parse2, Data, DeriveInput, Fields, Result};

/// Implementation of the #[derive(Accounts)] derive macro.
pub fn derive_accounts_impl(item: TokenStream) -> Result<TokenStream> {
    let input: DeriveInput = parse2(item)?;
    let name = &input.ident;
    let generics = &input.generics;

    // We expect a lifetime parameter like <'info>
    let lifetime = generics
        .lifetimes()
        .next()
        .map(|l| &l.lifetime)
        .ok_or_else(|| {
            syn::Error::new_spanned(
                &input,
                "Accounts struct must have a lifetime parameter, e.g., <'info>",
            )
        })?;

    let data_struct = match &input.data {
        Data::Struct(s) => s,
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "Accounts can only be derived for structs",
            ))
        }
    };

    let fields = match &data_struct.fields {
        Fields::Named(f) => f,
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "Accounts struct must have named fields",
            ))
        }
    };

    // Parse each field's #[account(...)] constraints
    let mut field_validations = Vec::new();
    let mut field_deserializations = Vec::new();
    let mut field_names = Vec::new();
    let mut bump_fields = Vec::new();

    for field in &fields.named {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;
        field_names.push(field_name.clone());

        let constraints = parse_account_constraints(&field.attrs)?;

        // Generate validation code based on constraints
        let mut validations = Vec::new();

        if constraints.is_signer {
            validations.push(quote! {
                if !#field_name.is_signer() {
                    return Err(dchat_dpl::DplError::MissingSigner);
                }
            });
        }

        if constraints.is_mut {
            validations.push(quote! {
                if !#field_name.is_writable() {
                    return Err(dchat_dpl::DplError::AccountNotMutable);
                }
            });
        }

        if let Some(ref constraint_expr) = constraints.constraint {
            let expr: syn::Expr = syn::parse_str(constraint_expr)?;
            validations.push(quote! {
                if !(#expr) {
                    return Err(dchat_dpl::DplError::ConstraintViolation);
                }
            });
        }

        if let Some(ref has_one_field) = constraints.has_one {
            let has_one_ident = format_ident!("{}", has_one_field);
            let error_expr = if let Some(ref error_path) = constraints.has_one_error {
                let path: syn::Path = syn::parse_str(error_path).unwrap();
                quote! { #path.into() }
            } else {
                quote! { dchat_dpl::DplError::ConstraintHasOne }
            };

            // Try to extract the inner account data type from Account<'info, T>
            let inner_ty: Option<syn::Type> = if let syn::Type::Path(tp) = field_type {
                tp.path.segments.last().and_then(|seg| {
                    if seg.ident == "Account" {
                        if let syn::PathArguments::AngleBracketed(ab) = &seg.arguments {
                            ab.args
                                .iter()
                                .filter_map(|ga| {
                                    if let syn::GenericArgument::Type(ty) = ga {
                                        Some(ty.clone())
                                    } else {
                                        None
                                    }
                                })
                                .nth(1)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            if let Some(inner) = inner_ty {
                validations.push(quote! {
                    let mut __buf = #field_name.data();
                    let __parsed: #inner = dchat_dpl::AccountDeserialize::try_deserialize(&mut __buf)?;
                    if __parsed.#has_one_ident != *#has_one_ident.key() {
                        return Err(#error_expr);
                    }
                });
            } else {
                validations.push(quote! {
                    if *#has_one_ident.key() != *#field_name.key() {
                        return Err(#error_expr);
                    }
                });
            }
        }

        // Handle PDA derivation with seeds and bump
        if !constraints.seeds.is_empty() {
            bump_fields.push(field_name.clone());
            let seeds_tokens: Vec<_> = constraints
                .seeds
                .iter()
                .map(|s| {
                    let expr: syn::Expr = syn::parse_str(s).unwrap();
                    quote! { #expr.as_ref() }
                })
                .collect();

            validations.push(quote! {
                if let Some((expected_key, bump)) = dchat_dpl::derive_pda(
                    &[#(#seeds_tokens),*],
                    &ctx.program_id,
                ) {
                    if *#field_name.key() != expected_key {
                        return Err(dchat_dpl::DplError::InvalidPda);
                    }
                    bumps.#field_name = bump;
                } else {
                    return Err(dchat_dpl::DplError::InvalidPda);
                }
            });
        }

        field_validations.push(quote! {
            #(#validations)*
        });

        // Generate deserialization based on field type
        field_deserializations.push(quote! {
            let mut __entry = cursor.next_account()?;
            let #field_name: #field_type = dchat_dpl::FromAccountEntry::from_entry(&mut __entry)?;
        });
    }

    // Generate Bumps struct for PDA bumps
    let bumps_struct_name = format_ident!("{}Bumps", name);
    let bump_field_defs: Vec<_> = bump_fields
        .iter()
        .map(|f| {
            quote! { pub #f: u8 }
        })
        .collect();
    let bump_field_defaults: Vec<_> = bump_fields
        .iter()
        .map(|f| {
            quote! { #f: 0 }
        })
        .collect();

    let bumps_struct = if bump_fields.is_empty() {
        quote! {
            #[derive(Debug, Default, Clone)]
            pub struct #bumps_struct_name;
        }
    } else {
        quote! {
            #[derive(Debug, Default, Clone)]
            pub struct #bumps_struct_name {
                #(#bump_field_defs,)*
            }

            impl #bumps_struct_name {
                pub fn new() -> Self {
                    Self {
                        #(#bump_field_defaults,)*
                    }
                }
            }
        }
    };

    let output = quote! {
        #bumps_struct

        impl<#lifetime> dchat_dpl::Accounts<#lifetime> for #name<#lifetime> {
            type Bumps = #bumps_struct_name;

            fn try_accounts(
                ctx: &dchat_dpl::ContextInfo,
                accounts_data: &#lifetime [u8],
                bumps: &mut Self::Bumps,
            ) -> dchat_dpl::Result<Self> {
                let mut cursor = dchat_dpl::abi::AccountsCursor::new(accounts_data)?;

                #(#field_deserializations)*

                // Validate constraints
                #(#field_validations)*

                Ok(Self {
                    #(#field_names,)*
                })
            }
        }
    };

    Ok(output)
}

/// Parsed account constraints from #[account(...)] attribute.
#[derive(Default)]
struct AccountConstraints {
    is_mut: bool,
    is_signer: bool,
    is_init: bool,
    payer: Option<String>,
    space: Option<String>,
    seeds: Vec<String>,
    bump: bool,
    has_one: Option<String>,
    has_one_error: Option<String>,
    constraint: Option<String>,
}

/// Parse #[account(...)] attributes into AccountConstraints.
fn parse_account_constraints(attrs: &[syn::Attribute]) -> Result<AccountConstraints> {
    let mut constraints = AccountConstraints::default();

    for attr in attrs {
        if !attr.path().is_ident("account") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("mut") {
                constraints.is_mut = true;
            } else if meta.path.is_ident("signer") {
                constraints.is_signer = true;
            } else if meta.path.is_ident("init") {
                constraints.is_init = true;
            } else if meta.path.is_ident("bump") {
                constraints.bump = true;
            } else if meta.path.is_ident("payer") {
                let value = meta.value()?;
                let lit: syn::Ident = value.parse()?;
                constraints.payer = Some(lit.to_string());
            } else if meta.path.is_ident("space") {
                let value = meta.value()?;
                let expr: syn::Expr = value.parse()?;
                constraints.space = Some(quote!(#expr).to_string());
            } else if meta.path.is_ident("has_one") {
                let value = meta.value()?;
                // Parse the field name
                let field_ident: syn::Ident = value.parse()?;
                constraints.has_one = Some(field_ident.to_string());
                // Check for optional @ Error syntax
                if meta.input.peek(syn::Token![@]) {
                    let _: syn::Token![@] = meta.input.parse()?;
                    let error_path: syn::Path = meta.input.parse()?;
                    constraints.has_one_error = Some(quote!(#error_path).to_string());
                }
            } else if meta.path.is_ident("constraint") {
                let value = meta.value()?;
                let expr: syn::Expr = value.parse()?;
                constraints.constraint = Some(quote!(#expr).to_string());
            } else if meta.path.is_ident("seeds") {
                // Support both `seeds = [..]` and `seeds[..]` syntaxes
                if meta.input.peek(syn::Token![=]) {
                    let _ = meta.value()?; // consume '=' and advance to bracket group
                }
                let content;
                syn::bracketed!(content in meta.input);
                let seeds: syn::punctuated::Punctuated<syn::Expr, syn::Token![,]> =
                    content.parse_terminated(syn::Expr::parse, syn::Token![,])?;
                constraints.seeds = seeds.iter().map(|e| quote!(#e).to_string()).collect();
            }
            Ok(())
        })?;
    }

    Ok(constraints)
}
