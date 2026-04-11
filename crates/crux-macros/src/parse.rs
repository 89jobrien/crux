/// Parse `#[crux::agent]` attribute arguments.
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token};

#[derive(Debug, Default)]
pub struct AgentArgs {
    pub registry: Option<String>,
    pub checkpoint_every_step: bool,
    pub replay: ReplayMode,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum ReplayMode {
    #[default]
    Strict,
    Lenient,
}

impl Parse for AgentArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = AgentArgs::default();

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "registry" => {
                    let _: Token![=] = input.parse()?;
                    let lit: LitStr = input.parse()?;
                    args.registry = Some(lit.value());
                }
                "checkpoint_every_step" => {
                    args.checkpoint_every_step = true;
                }
                "replay" => {
                    let _: Token![=] = input.parse()?;
                    let lit: LitStr = input.parse()?;
                    args.replay = match lit.value().as_str() {
                        "strict" => ReplayMode::Strict,
                        "lenient" => ReplayMode::Lenient,
                        other => {
                            return Err(syn::Error::new(
                                lit.span(),
                                format!("unknown replay mode: '{other}', expected 'strict' or 'lenient'"),
                            ));
                        }
                    };
                }
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!("unknown attribute: '{other}'"),
                    ));
                }
            }

            if !input.is_empty() {
                let _: Token![,] = input.parse()?;
            }
        }

        Ok(args)
    }
}
