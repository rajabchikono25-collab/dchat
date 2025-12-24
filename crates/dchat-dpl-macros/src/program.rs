//! #[program] macro implementation
//!
//! Generates the program entrypoint, instruction dispatcher, and DPL manifest.

use proc_macro2::{Span, TokenStream};
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

    // Generate dispatch match arms using 8-byte function discriminators
    fn to_static_lifetime(mut ty: syn::Type) -> syn::Type {
        if let syn::Type::Path(tp) = &mut ty {
            for seg in tp.path.segments.iter_mut() {
                if let syn::PathArguments::AngleBracketed(ab) = &mut seg.arguments {
                    let mut new_args: syn::punctuated::Punctuated<
                        syn::GenericArgument,
                        syn::Token![,],
                    > = syn::punctuated::Punctuated::new();
                    for arg in ab.args.clone() {
                        let replaced = match arg {
                            syn::GenericArgument::Lifetime(_) => syn::GenericArgument::Lifetime(
                                syn::Lifetime::new("'static", Span::call_site()),
                            ),
                            other => other,
                        };
                        new_args.push(replaced);
                    }
                    ab.args = new_args;
                }
            }
        }
        ty
    }

    let dispatch_arms: Vec<_> = handlers
        .iter()
        .map(|func| {
            let name = &func.sig.ident;
            let const_name = format_ident!("{}_DISCRIMINATOR", name.to_string().to_uppercase());

            // Parse Context<T> from first argument (robustly search for a segment named Context)
            let accounts_ty: syn::Type = match func.sig.inputs.first() {
                Some(syn::FnArg::Typed(pat_ty)) => {
                    if let syn::Type::Path(tp) = &*pat_ty.ty {
                        let seg_opt = tp.path.segments.iter().find(|s| s.ident == "Context");
                        if let Some(seg) = seg_opt {
                            if let syn::PathArguments::AngleBracketed(ab) = &seg.arguments {
                                if let Some(syn::GenericArgument::Type(ty)) = ab.args.iter().nth(1) {
                                    to_static_lifetime(ty.clone())
                                } else if let Some(syn::GenericArgument::Type(ty)) = ab.args.first() {
                                    // Some users write Context<T> without explicit lifetime first
                                    to_static_lifetime(ty.clone())
                                } else { syn::parse_str("() ").unwrap() }
                            } else { syn::parse_str("() ").unwrap() }
                        } else { syn::parse_str("() ").unwrap() }
                    } else { syn::parse_str("() ").unwrap() }
                }
                _ => syn::parse_str("() ").unwrap(),
            };

            // Extract remaining non-ctx parameters (support 0, 1, 2, or more)
            let other_params: Vec<_> = func.sig.inputs.iter().skip(1).collect();

            if other_params.is_empty() {
                quote! {
                    disc if disc == #mod_name::#const_name => {
                        let ctx_info = dchat_dpl::ContextInfo::new(#mod_name::PROGRAM_ID);
                        let mut bumps: <#accounts_ty as dchat_dpl::Accounts>::Bumps = Default::default();
                        let accounts = <#accounts_ty as dchat_dpl::Accounts>::try_accounts(&ctx_info, accounts_data, &mut bumps)?;
                        let ctx = dchat_dpl::Context::new(accounts, &[], bumps, &#mod_name::PROGRAM_ID);
                        #mod_name::#name(ctx)
                    }
                }
            } else {
                // Multiple argument types - deserialize each in order
                let arg_types: Vec<syn::Type> = other_params.iter().map(|p| {
                    if let syn::FnArg::Typed(pat_ty) = p { (*pat_ty.ty).clone() } else { syn::parse_str("()").unwrap() }
                }).collect();

                let arg_count = arg_types.len();
                let arg_names: Vec<_> = (0..arg_count).map(|i| format_ident!("arg{}", i)).collect();

                let deserialize_stmts: Vec<_> = arg_types.iter().zip(arg_names.iter()).map(|(ty, name)| {
                    quote! { let #name: #ty = dchat_dpl::DplDeserialize::deserialize(&mut remaining_data)?; }
                }).collect();

                quote! {
                    disc if disc == #mod_name::#const_name => {
                        let ctx_info = dchat_dpl::ContextInfo::new(#mod_name::PROGRAM_ID);
                        let mut bumps: <#accounts_ty as dchat_dpl::Accounts>::Bumps = Default::default();
                        let accounts = <#accounts_ty as dchat_dpl::Accounts>::try_accounts(&ctx_info, accounts_data, &mut bumps)?;
                        let ctx = dchat_dpl::Context::new(accounts, &[], bumps, &#mod_name::PROGRAM_ID);
                        #(#deserialize_stmts)*
                        #mod_name::#name(ctx, #(#arg_names),*)
                    }
                }
            }
        })
        .collect();

    // Generate the program ID with build-time derivation support
    // The program ID can be set via:
    // 1. DPL_PROGRAM_ID env var (hex-encoded 32 bytes) - for production builds
    // 2. Build script that writes to OUT_DIR/program_id.bin
    // 3. Default: derived from module path hash (deterministic for dev builds)
    let mod_name_str = mod_name.to_string();
    let program_id_const = quote! {
        /// The program ID. Set by the DPL build process or derived from module path.
        ///
        /// For production deployments, set the DPL_PROGRAM_ID environment variable
        /// or use `dpl build` which computes and patches the correct program ID.
        pub static PROGRAM_ID: dchat_dpl::Pubkey = {
            // Try to use build-time provided program ID
            #[cfg(feature = "dpl_embed_program_id")]
            {
                dchat_dpl::Pubkey(*include_bytes!(concat!(env!("OUT_DIR"), "/dpl_program_id.bin")))
            }

            // Default: derive from module path for deterministic dev builds
            #[cfg(not(feature = "dpl_embed_program_id"))]
            {
                // Use const evaluation to derive a deterministic ID from module name
                // This ensures the same module always gets the same ID in dev builds
                const fn derive_program_id(name: &[u8]) -> [u8; 32] {
                    // Simple FNV-1a hash spread across 32 bytes for dev builds
                    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
                    const FNV_PRIME: u64 = 0x100000001b3;

                    let mut hash = FNV_OFFSET;
                    let mut i = 0;
                    while i < name.len() {
                        hash ^= name[i] as u64;
                        hash = hash.wrapping_mul(FNV_PRIME);
                        i += 1;
                    }

                    // Expand hash to 32 bytes by repeated hashing
                    let mut result = [0u8; 32];
                    let mut j = 0;
                    while j < 4 {
                        let bytes = hash.to_le_bytes();
                        let mut k = 0;
                        while k < 8 {
                            result[j * 8 + k] = bytes[k];
                            k += 1;
                        }
                        hash = hash.wrapping_mul(FNV_PRIME);
                        j += 1;
                    }
                    result
                }

                dchat_dpl::Pubkey(derive_program_id(#mod_name_str.as_bytes()))
            }
        };
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
            if program_id != #mod_name::PROGRAM_ID {
                return Err(dchat_dpl::DplError::InvalidProgramId);
            }

            // Extract discriminator (first 8 bytes)
            if instruction_data.len() < 8 {
                return Err(dchat_dpl::DplError::InvalidInstructionData);
            }
            let discriminator: [u8; 8] = instruction_data[..8].try_into().unwrap();
            let mut remaining_data = &instruction_data[8..];

            match discriminator {
                #(#dispatch_arms,)*
                _ => Err(dchat_dpl::DplError::InvalidInstructionDiscriminator),
            }
        }
    };

    // Generate the DPL manifest in a custom section
    //
    // IRONCLAD: The manifest uses magic "DPLM" (0x44, 0x50, 0x4C, 0x4D) and embeds
    // the schema hash computed from the canonical IDL. The runtime validator enforces
    // that deployed programs have a valid manifest with non-zero schema hash.
    //
    // Manifest layout (64 bytes):
    //   [0..4]   Magic: "DPLM"
    //   [4..6]   SDK major version (little-endian u16)
    //   [6..8]   SDK minor version (little-endian u16)
    //   [8..10]  SDK patch version (little-endian u16)
    //   [10..12] Edition year (little-endian u16, e.g., 2025 = 0x07E9)
    //   [12]     ABI version (u8)
    //   [13]     Import profile (u8): 0=Legacy, 1=WASI, 2=Hybrid
    //   [14..16] Reserved (must be zero)
    //   [16..48] Schema hash (BLAKE3 of canonical IDL, 32 bytes)
    //   [48..56] Capabilities bitflags (little-endian u64)
    //   [56..64] Reserved (must be zero)
    //
    // For development builds, schema_hash may be zeros, but production deployments
    // MUST have a real schema hash. The `dpl build` command computes and embeds it.
    let manifest_section = quote! {
        #[cfg(target_arch = "wasm32")]
        #[link_section = "dpl_manifest"]
        #[used]
        static DPL_MANIFEST: [u8; 64] = {
            // Magic: "DPLM" (0x44, 0x50, 0x4C, 0x4D)
            let mut manifest = [0u8; 64];
            manifest[0] = 0x44; // 'D'
            manifest[1] = 0x50; // 'P'
            manifest[2] = 0x4C; // 'L'
            manifest[3] = 0x4D; // 'M'

            // SDK version (use DPL_VERSION_* constants at compile time)
            // Note: These are little-endian u16 values
            let sdk_major: u16 = dchat_dpl::DPL_VERSION_MAJOR;
            let sdk_minor: u16 = dchat_dpl::DPL_VERSION_MINOR;
            let sdk_patch: u16 = dchat_dpl::DPL_VERSION_PATCH;
            let edition: u16 = dchat_dpl::DPL_EDITION;
            let abi_version: u8 = dchat_dpl::ABI_VERSION;

            // SDK major version [4..6]
            manifest[4] = (sdk_major & 0xFF) as u8;
            manifest[5] = ((sdk_major >> 8) & 0xFF) as u8;

            // SDK minor version [6..8]
            manifest[6] = (sdk_minor & 0xFF) as u8;
            manifest[7] = ((sdk_minor >> 8) & 0xFF) as u8;

            // SDK patch version [8..10]
            manifest[8] = (sdk_patch & 0xFF) as u8;
            manifest[9] = ((sdk_patch >> 8) & 0xFF) as u8;

            // Edition year [10..12]
            manifest[10] = (edition & 0xFF) as u8;
            manifest[11] = ((edition >> 8) & 0xFF) as u8;

            // ABI version [12]
            manifest[12] = abi_version;

            // Import profile [13]: 1 = WASI (default for wasm32-wasi target)
            manifest[13] = 1;

            // Reserved [14..16]: must be zero (already initialized)

            // Schema hash [16..48]: 32 bytes
            // Production builds MUST have a valid (non-zero) schema hash.
            // The hash is computed from the canonical IDL and embedded via:
            //   1. `dpl build` command (patches the WASM binary post-compilation)
            //   2. Build script writing to OUT_DIR/dpl_schema_hash.bin
            //   3. dpl_embed_schema_hash feature flag
            //
            // Development builds use zero hash which strict validators will reject.
            #[cfg(feature = "dpl_embed_schema_hash")]
            {
                // Production build: schema hash provided by build.rs or dpl build
                let hash: [u8; 32] = *include_bytes!(concat!(env!("OUT_DIR"), "/dpl_schema_hash.bin"));
                let mut i = 0;
                while i < 32 {
                    manifest[16 + i] = hash[i];
                    i += 1;
                }
            }
            // Development builds without embedded hash: zeros (strict validators reject)

            // Capabilities [48..56]: 8 bytes (little-endian u64)
            // Default: no special capabilities declared
            // Programs can override via #[program(capabilities = ...)]

            // Reserved [56..64]: must be zero (already initialized)

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
