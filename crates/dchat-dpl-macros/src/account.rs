//! #[account] attribute macro implementation
//!
//! Generates account discriminator, serialization, and space calculation.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse2, Data, DeriveInput, Fields, Result};

/// Implementation of the #[account] attribute macro.
pub fn account_impl(_attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
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
                "#[account] can only be applied to structs",
            ))
        }
    };

    let fields = match &data_struct.fields {
        Fields::Named(f) => f.named.iter().collect::<Vec<_>>(),
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "#[account] struct must have named fields",
            ))
        }
    };

    let field_names: Vec<_> = fields.iter().map(|f| &f.ident).collect();
    let field_types: Vec<_> = fields.iter().map(|f| &f.ty).collect();

    // Generate the struct definition with derives
    let struct_def = quote! {
        #(#attrs)*
        #[derive(Debug, Clone, borsh::BorshSerialize, borsh::BorshDeserialize)]
        #vis struct #name #generics #where_clause {
            #(#vis #field_names: #field_types,)*
        }
    };

    // Generate discriminator constant
    let discriminator = quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            /// The 8-byte discriminator for this account type.
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
                fnv_hash(concat!("account:", #name_str).as_bytes())
            };

            /// Returns the discriminator for this account type.
            pub fn discriminator() -> [u8; 8] {
                Self::DISCRIMINATOR
            }
        }
    };

    // Generate AccountSerialize trait
    let serialize_impl = quote! {
        impl #impl_generics dchat_dpl::AccountSerialize for #name #ty_generics #where_clause {
            fn try_serialize(&self, writer: &mut impl std::io::Write) -> dchat_dpl::Result<()> {
                // Write discriminator first
                writer.write_all(&Self::DISCRIMINATOR)?;
                // Then serialize the data
                borsh::BorshSerialize::serialize(self, writer)?;
                Ok(())
            }
        }
    };

    // Generate AccountDeserialize trait
    let deserialize_impl = quote! {
        impl #impl_generics dchat_dpl::AccountDeserialize for #name #ty_generics #where_clause {
            fn try_deserialize(buf: &mut &[u8]) -> dchat_dpl::Result<Self> {
                // Verify discriminator
                if buf.len() < 8 {
                    return Err(dchat_dpl::DplError::AccountDataTooSmall);
                }
                let disc: [u8; 8] = buf[..8].try_into().unwrap();
                if disc != Self::DISCRIMINATOR {
                    return Err(dchat_dpl::DplError::AccountDiscriminatorMismatch);
                }
                *buf = &buf[8..];

                // Deserialize the data
                let account = borsh::BorshDeserialize::deserialize(buf)?;
                Ok(account)
            }
        }
    };

    // Generate space calculation
    let space_impl = quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            /// Calculate the space needed for this account (excluding discriminator).
            /// Note: For dynamic types, this returns the minimum size.
            pub const fn space() -> usize {
                8 // discriminator
                #(+ std::mem::size_of::<#field_types>())*
            }
        }
    };

    let output = quote! {
        #struct_def
        #discriminator
        #serialize_impl
        #deserialize_impl
        #space_impl
    };

    Ok(output)
}
