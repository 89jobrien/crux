//! Bridge plugin handlers into `HandlerRegistry`.
//!
//! Wraps `PluginHost` in shared state so that type-erased handler
//! closures can invoke plugins at runtime.

use std::sync::Arc;

use crate::host::{PluginError, PluginHost};
use crate::manifest::PluginEntry;
use crate::protocol::HandlerDecl;
use crux_runtime::prelude::CruxErr;
use crux_script::{HandlerMetadata, HandlerRegistry, ValueSchema};

/// Load all plugins from the given entries and register their
/// handlers into the registry.
pub async fn register_plugins(
    registry: &mut HandlerRegistry,
    entries: &[PluginEntry],
) -> Result<(), PluginError> {
    let mut host = PluginHost::new();
    for entry in entries {
        if let Err(e) = host.load_plugin(entry).await {
            host.shutdown_all().await;
            return Err(e);
        }
    }

    let declarations = host.declared_handlers().to_vec();

    let mut metadatas = Vec::with_capacity(declarations.len());
    for decl in &declarations {
        match metadata_for(decl) {
            Ok(metadata) => metadatas.push((decl.name.clone(), metadata)),
            Err(e) => {
                host.shutdown_all().await;
                return Err(e);
            }
        }
    }

    let host = Arc::new(host);

    for (name, metadata) in metadatas {
        let host = host.clone();
        registry.handler_value_with_metadata(metadata, move |input: serde_json::Value| {
            let host = host.clone();
            let name = name.clone();
            async move {
                host.invoke(&name, input)
                    .await
                    .map_err(|e| CruxErr::step_failed(&name, e.to_string()))
            }
        });
    }

    Ok(())
}

/// Lift one plugin declaration into handler metadata.
///
/// An undeclared result shape becomes an explicit `Dynamic` schema, which is
/// the same contract the compiler would have inferred from absent metadata.
/// A declared one is checked for recursive validity first: the schema arrives
/// over the wire from a subprocess, and a malformed one has to fail at load
/// time rather than midway through a run.
fn metadata_for(decl: &HandlerDecl) -> Result<HandlerMetadata, PluginError> {
    let output_schema = decl.output_schema.clone().unwrap_or(ValueSchema::Dynamic);
    output_schema
        .validate_definition()
        .map_err(|source| PluginError::InvalidSchema {
            handler: decl.name.clone(),
            source,
        })?;

    Ok(HandlerMetadata::new(&decl.name)
        .describe(&decl.description)
        .output_schema(output_schema))
}
