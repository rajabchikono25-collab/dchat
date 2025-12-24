//! #[program] macro implementation
//!
//! Generates the program entrypoint, instruction dispatcher, and DPL manifest.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse2, ItemMod, Result, Visibility};

/// Implementation of the #[program] attribute macro.
pub fn program_impl(_attr: TokenStream, item: TokenStream) -> Result<TokenStream> {
    let module: ItemMod = parse2(item)?;
    let mod_name = &module.ident;
    let mod_vis = &module.vis;

    // Extract the module content
    let content = match &module.content {
        Some((_, items)) => items,
        None => {
            return Err(syn::Error::new_spanned(
                &module,
                "#[program] requires an inline module definition",
            ))
        }
    };

    // Collect instruction handlers (public functions)
    let mut handlers = Vec::new();
    let mut handler_names = Vec::new();

    for item in content.iter() {
        if let syn::Item::Fn(func) = item {
            if matches!(func.vis, Visibility::Public(_)) {
                handlers.push(func.clone());
                handler_names.push(func.sig.ident.clone());
            }
        }
    }

    // Generate instruction discriminators (first 8 bytes of blake3 hash)
    let discriminator_consts: Vec<_> = handler_names
        .iter()
        .map(|name| {
            let name_str = name.to_string();
            let const_name = format_ident!("{}_DISCRIMINATOR", name.to_string().to_uppercase());
            quote! {
                pub const #const_name: [u8; 8] = {
                    // Compile-time discriminator: first 8 bytes of namespace:name hash
                    // At compile time we use a simple hash; runtime uses blake3
                    const fn simple_hash(s: &[u8]) -> [u8; 8] {
                        let mut h: u64 = 0xcbf29ce484222325; // FNV offset
                        let mut i = 0;
                        while i < s.len() {
                            h ^= s[i] as u64;
                            h = h.wrapping_mul(0x100000001b3); // FNV prime
                            i += 1;
                        }
                        h.to_le_bytes()
                    }
                    simple_hash(concat!("global:", #name_str).as_bytes())
                };
            }
        })
        .collect();

    // Generate dispatch match arms
    let dispatch_arms: Vec<_> = handler_names
        .iter()
        .zip(handlers.iter())
        .map(|(name, func)| {
            let const_name = format_ident!("{}_DISCRIMINATOR", name.to_string().to_uppercase());

            // Extract function parameters (skip ctx)
            let params: Vec<_> = func
                .sig
                .inputs
                .iter()
                .skip(1) // Skip Context parameter
                .collect();

            if params.is_empty() {
                quote! {
                    disc if disc == #const_name => {
                        let ctx = dchat_dpl::Context::new(&accounts_data, &mut remaining_data)?;
                        #mod_name::#name(ctx)
                    }
                }
            } else {
                quote! {
                    disc if disc == #const_name => {
                        let ctx = dchat_dpl::Context::new(&accounts_data, &mut remaining_data)?;
                        // Deserialize remaining instruction data as arguments
                        let args = dchat_dpl::DplDeserialize::deserialize(&mut remaining_data)?;
                        #mod_name::#name(ctx, args)
                    }
                }
            }
        })
        .collect();

    // Generate the program ID placeholder (would be set by build script in real impl)
    let program_id_const = quote! {
        /// The program ID. Set by the DPL build process.
        pub static PROGRAM_ID: dchat_dpl::Pubkey = dchat_dpl::Pubkey([0u8; 32]);
    };

    // Generate the entrypoint
    let entrypoint = quote! {
        /// Program entrypoint called by the VM.
        #[no_mangle]
        pub extern "C" fn entrypoint(input: *mut u8) -> u64 {
            match __dpl_process_instruction(input) {
                Ok(()) => 0,
                Err(e) => e.into(),
            }
        }

        fn __dpl_process_instruction(input: *mut u8) -> dchat_dpl::Result<()> {
            // Parse the instruction envelope
            let (program_id, accounts_data, instruction_data) =
                unsafe { dchat_dpl::abi::parse_input(input)? };

            // Verify we're being called with our program ID
            if program_id != PROGRAM_ID {
                return Err(dchat_dpl::DplError::InvalidProgramId);
            }

            // Extract discriminator (first 8 bytes)
            if instruction_data.len() < 8 {
                return Err(dchat_dpl::DplError::InvalidInstructionData);
            }
            let discriminator: [u8; 8] = instruction_data[..8].try_into().unwrap();
            let mut remaining_data = &instruction_data[8..];

            // Dispatch to handler
            match discriminator {
                #(#dispatch_arms,)*
                _ => Err(dchat_dpl::DplError::InvalidInstructionDiscriminator),
            }
        }
    };

    // Generate the DPL manifest in a custom section
    let manifest_section = quote! {
        #[cfg(target_arch = "wasm32")]
        #[link_section = "dpl_manifest"]
        #[used]
        static DPL_MANIFEST: [u8; 64] = {
            // Magic: "DPL\0"
            let mut manifest = [0u8; 64];
            manifest[0] = b'D';
            manifest[1] = b'P';
            manifest[2] = b'L';
            manifest[3] = 0;
            // Version: 0.1.0
            manifest[4] = 0; // major high
            manifest[5] = 1; // major low
            manifest[6] = 0; // minor high
            manifest[7] = 0; // minor low
            manifest[8] = 0; // patch high
            manifest[9] = 0; // patch low
            // Edition: 2025
            manifest[10] = 0x07;
            manifest[11] = 0xe9;
            // ABI version: 1
            manifest[12] = 1;
            // Import profile: 1 (WASI)
            manifest[13] = 1;
            // Reserved
            manifest[14] = 0;
            manifest[15] = 0;
            // Schema hash (placeholder - computed at build time)
            // Bytes 16-47: 32 bytes of zeros
            // Capabilities (placeholder)
            // Bytes 48-55: 8 bytes
            // Reserved
            // Bytes 56-63: 8 bytes
            manifest
        };
    };

    // Reconstruct the module with all additions
    let original_items = content.iter();

    let output = quote! {
        #mod_vis mod #mod_name {
            use super::*;

            #program_id_const

            #(#discriminator_consts)*

            #(#original_items)*
        }

        #entrypoint

        #manifest_section
    };

    Ok(output)
}
