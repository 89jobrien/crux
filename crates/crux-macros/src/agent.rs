/// Code generation for `#[crux::agent]`.
///
/// Transforms:
///   #[crux::agent]
///   async fn hello(name: String) -> Crux<String> { ... }
///
/// Into:
///   - Inner function with CruxCtx as first param, aliased to `t`
///   - Public wrapper that creates CruxCtx and calls finalize()
///   - HelloAgent struct implementing Agent trait
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse2, ItemFn};

use crate::parse::AgentArgs;

pub fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let args: AgentArgs = parse2(attr)?;
    let func: ItemFn = parse2(item)?;

    let fn_name = &func.sig.ident;
    let fn_vis = &func.vis;
    let fn_block = &func.block;
    let fn_inputs = &func.sig.inputs;

    // Extract the T from -> Crux<T>
    let output_type = extract_crux_inner_type(&func.sig.output)?;

    // Generate agent struct name: hello -> HelloAgent
    let agent_struct = format_ident!(
        "{}Agent",
        to_pascal_case(&fn_name.to_string())
    );

    let _inner_fn = format_ident!("__crux_{}_inner", fn_name);

    // Collect param names and types for forwarding
    let param_names: Vec<_> = func
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let syn::FnArg::Typed(pat) = arg {
                Some(&pat.pat)
            } else {
                None
            }
        })
        .collect();

    let param_types: Vec<_> = func
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let syn::FnArg::Typed(pat) = arg {
                Some(&pat.ty)
            } else {
                None
            }
        })
        .collect();

    // For single param, Input = that type. For multiple, use a tuple.
    let (input_type, input_destructure, input_forward) = match param_types.len() {
        0 => (
            quote! { () },
            quote! { () },
            quote! { () },
        ),
        1 => {
            let ty = &param_types[0];
            let name = &param_names[0];
            (
                quote! { #ty },
                quote! { #name },
                quote! { #name },
            )
        }
        _ => {
            let types = &param_types;
            let names = &param_names;
            (
                quote! { (#(#types),*) },
                quote! { (#(#names),*) },
                quote! { (#(#names),*) },
            )
        }
    };

    let _ = &args; // reserved for registry/checkpoint/replay in later phases

    Ok(quote! {
        #fn_vis struct #agent_struct;

        impl ::crux_core::agent::Agent for #agent_struct {
            type Input = #input_type;
            type Output = #output_type;

            fn name() -> &'static str {
                stringify!(#fn_name)
            }

            async fn run(
                __crux_ctx: &mut ::crux_core::ctx::CruxCtx,
                #input_destructure: Self::Input,
            ) -> ::core::result::Result<Self::Output, ::crux_core::types::error::CruxErr> {
                let t = __crux_ctx;
                #fn_block
            }
        }

        #fn_vis async fn #fn_name(#fn_inputs) -> ::crux_core::types::crux_value::Crux<#output_type> {
            let mut __crux_ctx = ::crux_core::ctx::CruxCtx::new(stringify!(#fn_name));
            let __crux_result = <#agent_struct as ::crux_core::agent::Agent>::run(
                &mut __crux_ctx,
                #input_forward,
            ).await;
            __crux_ctx.finalize(__crux_result)
        }
    })
}

fn extract_crux_inner_type(
    output: &syn::ReturnType,
) -> syn::Result<TokenStream> {
    match output {
        syn::ReturnType::Default => Err(syn::Error::new_spanned(
            output,
            "#[crux::agent] functions must return Crux<T>",
        )),
        syn::ReturnType::Type(_, ty) => {
            // Parse Crux<T> and extract T
            if let syn::Type::Path(type_path) = ty.as_ref() {
                let last_segment = type_path
                    .path
                    .segments
                    .last()
                    .ok_or_else(|| syn::Error::new_spanned(ty, "expected Crux<T>"))?;

                if last_segment.ident != "Crux" {
                    return Err(syn::Error::new_spanned(
                        &last_segment.ident,
                        "expected Crux<T> return type",
                    ));
                }

                if let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return Ok(quote! { #inner });
                    }
                }
            }

            Err(syn::Error::new_spanned(ty, "expected Crux<T> return type"))
        }
    }
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + &chars.collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}
