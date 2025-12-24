//! #[event] attribute macro implementation
//!
//! Generates event discriminator and emission helpers.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse2, Data, DeriveInput, Fields, Result};

/// Implementation of the #[event] attribute macro.
pub fn event_impl(_attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
    let input: DeriveInput = parse2(item)?;
    let name = &input.ident;
    let name_str = name.to_string();
    let vis = &input.vis;
    let attrs = &input.attrs;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let data_struct = match &input.data {
        Data::Struct(s) => s,
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "#[event] can only be applied to structs",
            ))
        }
    };

    let fields = match &data_struct.fields {
        Fields::Named(f) => f.named.iter().collect::<Vec<_>>(),
        Fields::Unit => Vec::new(),
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "#[event] struct must have named fields or be a unit struct",
            ))
        }
    };

    let field_defs: Vec<_> = fields
        .iter()
        .map(|f| {
            let name = &f.ident;
            let ty = &f.ty;
            let field_vis = &f.vis;
            quote! { #field_vis #name: #ty }
        })
        .collect();

    // Generate the struct definition with derives
    let struct_def = if field_defs.is_empty() {
        quote! {
            #(#attrs)*
            #[derive(Debug, Clone, borsh::BorshSerialize, borsh::BorshDeserialize)]
            #vis struct #name #generics;
        }
    } else {
        quote! {
            #(#attrs)*
            #[derive(Debug, Clone, borsh::BorshSerialize, borsh::BorshDeserialize)]
            #vis struct #name #generics #where_clause {
                #(#field_defs,)*
            }
        }
    };

    // Generate event discriminator
    let discriminator = quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            /// The 8-byte discriminator for this event type.
            pub const DISCRIMINATOR: [u8; 8] = {
                const fn fnv_hash(s: &[u8]) -> [u8; 8] {
                    let mut h: u64 = 0xcbf29ce484222325;
                    let mut i = 0;
                    while i < s.len() {
                        h ^= s[i] as u64;
                        h = h.wrapping_mul(0x100000001b3);
                        i += 1;
                    }
                    h.to_le_bytes()
                }
                fnv_hash(concat!("event:", #name_str).as_bytes())
            };
        }
    };

    // Implement Event trait
    let event_impl = quote! {
        impl #impl_generics dchat_dpl::Event for #name #ty_generics #where_clause {
            fn discriminator() -> [u8; 8] {
                Self::DISCRIMINATOR
            }
        }
    };

    // Generate emit helper
    let emit_helper = quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            /// Emit this event to the transaction log.
            pub fn emit(&self) {
                dchat_dpl::emit_event(self);
            }
        }
    };

    let output = quote! {
        #struct_def
        #discriminator
        #event_impl
        #emit_helper
    };

    Ok(output)
}
