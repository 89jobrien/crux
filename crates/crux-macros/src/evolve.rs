use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{ItemFn, parse2};

use crate::agent::to_pascal_case;

/// `#[crux::evolve]` is syntactic sugar over `#[crux::agent]` that additionally
/// marks the generated agent struct as an evolution agent.
pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let func: ItemFn = parse2(item.clone())?;
    let fn_name = &func.sig.ident;
    let agent_struct = format_ident!("{}Agent", to_pascal_case(&fn_name.to_string()));

    // Delegate to agent::expand for the core generation
    let base = crate::agent::expand(attr, item)?;

    // Add an evolution marker impl
    let extended = quote! {
        #base

        impl #agent_struct {
            /// Marker: this agent was generated with `#[crux::evolve]`.
            pub fn is_evolution_agent() -> bool {
                true
            }
        }
    };

    Ok(extended)
}
