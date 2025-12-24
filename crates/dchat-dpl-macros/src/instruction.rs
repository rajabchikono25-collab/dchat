//! #[derive(Instruction)] macro implementation
//!
//! Generates instruction serialization/deserialization for instruction enums.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse2, Data, DeriveInput, Fields, Result};

/// Implementation of the #[derive(Instruction)] derive macro.
pub fn derive_instruction_impl(item: TokenStream) -> Result<TokenStream> {
    let input: DeriveInput = parse2(item)?;
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let data_enum = match &input.data {
        Data::Enum(e) => e,
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "Instruction can only be derived for enums",
            ))
        }
    };

    // Parse variants and their tags
    let mut serialize_arms = Vec::new();
    let mut deserialize_arms = Vec::new();

    for variant in &data_enum.variants {
        let variant_name = &variant.ident;

        // Parse #[tag(N)] attribute
        let tag = get_tag_attr(&variant.attrs)?;

        match &variant.fields {
            Fields::Named(fields) => {
                let field_names: Vec<_> = fields.named.iter().map(|f| &f.ident).collect();
                let field_names_clone = field_names.clone();

                serialize_arms.push(quote! {
                    #name::#variant_name { #(#field_names),* } => {
                        output.push(#tag);
                        #(
                            dchat_dpl::DplSerialize::serialize(#field_names, output)?;
                        )*
                        Ok(())
                    }
                });

                deserialize_arms.push(quote! {
                    #tag => {
                        Ok(#name::#variant_name {
                            #(
                                #field_names_clone: dchat_dpl::DplDeserialize::deserialize(data)?,
                            )*
                        })
                    }
                });
            }
            Fields::Unnamed(fields) => {
                let field_indices: Vec<_> = (0..fields.unnamed.len())
                    .map(|i| syn::Index::from(i))
                    .collect();
                let field_bindings: Vec<_> = (0..fields.unnamed.len())
                    .map(|i| quote::format_ident!("f{}", i))
                    .collect();
                let field_bindings_clone = field_bindings.clone();

                serialize_arms.push(quote! {
                    #name::#variant_name(#(#field_bindings),*) => {
                        output.push(#tag);
                        #(
                            dchat_dpl::DplSerialize::serialize(#field_bindings_clone, output)?;
                        )*
                        Ok(())
                    }
                });

                let _ = field_indices; // silence warning
                let deserialize_fields: Vec<_> = (0..fields.unnamed.len())
                    .map(|_| quote! { dchat_dpl::DplDeserialize::deserialize(data)? })
                    .collect();

                deserialize_arms.push(quote! {
                    #tag => {
                        Ok(#name::#variant_name(#(#deserialize_fields),*))
                    }
                });
            }
            Fields::Unit => {
                serialize_arms.push(quote! {
                    #name::#variant_name => {
                        output.push(#tag);
                        Ok(())
                    }
                });

                deserialize_arms.push(quote! {
                    #tag => Ok(#name::#variant_name),
                });
            }
        }
    }

    let output = quote! {
        impl #impl_generics dchat_dpl::DplSerialize for #name #ty_generics #where_clause {
            fn serialize(&self, output: &mut Vec<u8>) -> dchat_dpl::Result<()> {
                match self {
                    #(#serialize_arms)*
                }
            }
        }

        impl #impl_generics dchat_dpl::DplDeserialize for #name #ty_generics #where_clause {
            fn deserialize(data: &mut &[u8]) -> dchat_dpl::Result<Self> {
                if data.is_empty() {
                    return Err(dchat_dpl::DplError::InvalidInstructionData);
                }
                let tag = data[0];
                *data = &data[1..];

                match tag {
                    #(#deserialize_arms)*
                    _ => Err(dchat_dpl::DplError::InvalidInstructionDiscriminator),
                }
            }
        }
    };

    Ok(output)
}

/// Extracts the tag value from #[tag(N)] attribute.
fn get_tag_attr(attrs: &[syn::Attribute]) -> Result<u8> {
    for attr in attrs {
        if attr.path().is_ident("tag") {
            let lit: syn::LitInt = attr.parse_args()?;
            return Ok(lit.base10_parse()?);
        }
    }
    Err(syn::Error::new(
        proc_macro2::Span::call_site(),
        "Instruction variants must have a #[tag(N)] attribute",
    ))
}
