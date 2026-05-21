use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse2};

pub fn expand(_attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let input: DeriveInput = parse2(item)?;
    let name = &input.ident;
    let vis = &input.vis;

    // Extract fields (only works on named structs)
    let fields = match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            syn::Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "#[crux::harness] requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "#[crux::harness] can only be applied to structs",
            ));
        }
    };

    let field_defs = fields.iter();

    Ok(quote! {
        #[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
        #vis struct #name {
            #(#field_defs,)*
        }

        impl Default for #name {
            fn default() -> Self {
                Self {
                    memory_mb: 512,
                    cpu_millicores: 1000,
                    timeout_seconds: 300,
                    network_access: false,
                }
            }
        }

        impl #name {
            /// Convert this harness config into a `HarnessProfile`.
            pub fn to_profile(&self, id: &str) -> ::crux_runtime::types::harness::HarnessProfile {
                ::crux_runtime::types::harness::HarnessProfile {
                    id: id.to_string(),
                    resources: ::crux_runtime::types::harness::ResourceHints {
                        memory_mb: self.memory_mb,
                        cpu_millicores: self.cpu_millicores,
                        timeout_seconds: self.timeout_seconds,
                    },
                    network_access: self.network_access,
                    allowed_syscalls: Vec::new(),
                }
            }
        }
    })
}
