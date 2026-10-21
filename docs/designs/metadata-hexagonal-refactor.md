# Hexagonal Architecture Refactor: crux-script Metadata

## Executive Summary

The current `crux-script/src/metadata.rs` mixes domain metadata concepts with validation logic, serialization constraints, and error handling. This creates tight coupling that makes it difficult to:

- Add new validators without modifying domain types
- Swap serialization formats (YAML, JSON, protobuf)
- Test validation logic independently
- Extend metadata without structural changes

This design decouples metadata into layers:

1. **Domain Layer**: Pure metadata types (no validation, no serde)
2. **Validation Port**: Trait abstraction for validators
3. **Serialization Adapters**: Serde-based deserializers with validation baked in
4. **Validator Adapters**: Pluggable validators (strict, lenient, custom)

---

## Current State Analysis

### Coupling Points

| Issue | Location | Impact |
|-------|----------|--------|
| Validation logic embedded in types | `ValueSchema::validate_at()` | Can't skip validation, can't customize |
| Serde attributes hardcoded | `#[serde(tag = "type", ...)]` | Locked to one serialization format |
| Error types tied to validators | `SchemaViolation` defined with validator | Can't change error representation |
| Conversion logic in domain | `ArgType::from(ValueSchema)` | Business rules coupled to type system |
| Recursive validation deeply nested | `validate()` method | Hard to test, hard to extend |

### Example: ValueSchema

Current implementation:

```rust
// Domain + Validation + Serialization = Tight Coupling
pub enum ValueSchema {
    #[serde(tag = "type", content = "definition", rename_all = "snake_case")]
    Array { items: Box<ValueSchema> },
    Object(ObjectSchema),
    Union { variants: Vec<ValueSchema> },
}

impl ValueSchema {
    // Validation logic embedded (can't be swapped)
    pub fn validate(&self, value: &Value) -> Result<(), SchemaViolation> {
        self.validate_at(value, "$")
    }

    fn validate_at(&self, value: &Value, path: &str) -> Result<(), SchemaViolation> {
        // 50+ lines of validation code
    }

    // More validation methods...
    pub fn is_assignable_from(&self, source: &Self) -> bool { ... }
}
```

Problems:
- Can't use `ValueSchema` without `SchemaViolation`
- Can't validate differently (e.g., lenient vs strict mode)
- Can't test validators independently
- Serialization format locked in with serde attributes

---

## Proposed Architecture

### Layer Structure

```
┌──────────────────────────────────────────────┐
│        Composition Root (main.rs)            │
│   Wires validators, serializers to domain    │
└────────────────┬─────────────────────────────┘
                 │
    ┌────────────┴────────────┬──────────────┐
    │                         │              │
┌───▼──────────┐  ┌──────────▼──┐  ┌──────▼────────┐
│   Domain     │  │Validation   │  │Serialization  │
│   Layer      │  │Adapters     │  │Adapters       │
│              │  │             │  │               │
│ • ValueSchema│  │ • Validator │  │ • Serde       │
│ • ArgSchema  │  │   trait     │  │   deserializer│
│ • Metadata   │  │             │  │ • Custom      │
│              │  │ • Strict    │  │   format      │
│ (Pure types) │  │ • Lenient   │  │   handlers    │
│ (No logic)   │  │ • Custom    │  │               │
└──────────────┘  └─────────────┘  └────────────────┘
```

### Domain Layer: Pure Types

Remove validation logic, serialization constraints, and error coupling.

```rust
// domain.rs — Zero dependencies beyond serde, chrono, serde_json

/// JSON schema definition (no validation, no serde attributes)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueSchema {
    Dynamic,
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array { items: Box<ValueSchema> },
    Object(ObjectSchema),
    Union { variants: Vec<ValueSchema> },
}

impl ValueSchema {
    // Only construction and introspection, no validation
    pub fn array(items: ValueSchema) -> Self {
        Self::Array {
            items: Box::new(items),
        }
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array { .. })
    }

    // Removed: validate_at(), is_assignable_from(), validate()
}

#[derive(Debug, Clone)]
pub struct ObjectSchema {
    pub properties: BTreeMap<String, SchemaProperty>,
    pub additional: Option<Box<ValueSchema>>,
}

/// Handler metadata (domain only, no validation)
#[derive(Debug, Clone)]
pub struct HandlerMetadata {
    pub name: String,
    pub description: String,
    pub feature_flag: Option<String>,
    pub examples: Vec<JsonValue>,
    pub args: ArgSchema,
    pub input_schema: Option<ValueSchema>,
    pub output_schema: Option<ValueSchema>,
    pub confidence: Option<ConfidenceCapability>,
    pub risk: RiskLevel,
    pub side_effects: Vec<SideEffect>,
    pub capabilities: Vec<Capability>,
    pub deterministic: bool,
    pub replay_safe: bool,
}
```

**Key principles**:
- No serde attributes (serialization is an adapter concern)
- No validation methods (validation is a port)
- No embedded error types (errors defined in validators)
- Pure data + construction helpers only

### Validation Port: Trait Abstraction

Define a trait that validators implement.

```rust
// validation/mod.rs — Port definition

/// Error types live in the validator adapter, not domain
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaViolation {
    pub path: String,
    pub expected: ValueSchema,
    pub kind: SchemaViolationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaViolationKind {
    TypeMismatch { actual: ValueKind },
    MissingRequiredProperty,
    AdditionalPropertyNotAllowed,
}

/// Port: Schema validator
pub trait SchemaValidator {
    /// Validate a JSON value against a schema.
    fn validate(&self, schema: &ValueSchema, value: &JsonValue) -> Result<(), SchemaViolation>;

    /// Check whether `source` schema can flow into `target` schema.
    fn is_assignable_from(&self, target: &ValueSchema, source: &ValueSchema) -> bool;

    /// Validate that a schema definition itself is well-formed.
    fn validate_definition(&self, schema: &ValueSchema) -> Result<(), SchemaBuildError>;
}

/// Port: Capability authorizer
pub trait CapabilityAuthorizer {
    fn authorize(
        &self,
        handler: &HandlerMetadata,
        approved: &[Capability],
    ) -> Result<(), CapabilityViolation>;
}

/// Port: Definition builder/normalizer
pub trait MetadataBuilder {
    fn normalize_union(&self, variants: Vec<ValueSchema>) -> Result<ValueSchema, SchemaBuildError>;
}
```

**Benefits**:
- Multiple validators can coexist (strict, lenient, custom)
- Error types decoupled from domain
- Easy to test with mock validators
- Easy to add new validation strategies

### Validator Adapters

Implement the validator port for different strategies.

```rust
// validation/adapters/strict.rs

use super::{SchemaValidator, SchemaViolation, SchemaBuildError};
use crate::domain::{ValueSchema, ObjectSchema};
use serde_json::Value as JsonValue;

/// Strict validator: rejects ambiguous schemas, strict type matching
pub struct StrictValidator;

impl SchemaValidator for StrictValidator {
    fn validate(&self, schema: &ValueSchema, value: &JsonValue) -> Result<(), SchemaViolation> {
        self.validate_at(schema, value, "$")
    }

    fn is_assignable_from(&self, target: &ValueSchema, source: &ValueSchema) -> bool {
        match (target, source) {
            (ValueSchema::Dynamic, _) => true,
            (ValueSchema::Number, ValueSchema::Integer) => true,
            (ValueSchema::Array { items: t }, ValueSchema::Array { items: s }) => {
                self.is_assignable_from(t, s)
            }
            // ... strict rules
            _ => target == source,
        }
    }

    fn validate_definition(&self, schema: &ValueSchema) -> Result<(), SchemaBuildError> {
        match schema {
            ValueSchema::Union { variants } if variants.is_empty() => {
                Err(SchemaBuildError::EmptyUnion)
            }
            // ... recursively validate nested schemas
            _ => Ok(()),
        }
    }
}

impl StrictValidator {
    fn validate_at(
        &self,
        schema: &ValueSchema,
        value: &JsonValue,
        path: &str,
    ) -> Result<(), SchemaViolation> {
        let matches = match schema {
            ValueSchema::String => value.is_string(),
            ValueSchema::Array { items } => {
                if let Some(array) = value.as_array() {
                    for (index, item) in array.iter().enumerate() {
                        self.validate_at(items, item, &format!("{path}[{index}]"))?;
                    }
                    true
                } else {
                    false
                }
            }
            // ... all cases
        };

        if matches {
            Ok(())
        } else {
            Err(SchemaViolation {
                path: path.to_string(),
                expected: schema.clone(),
                kind: SchemaViolationKind::TypeMismatch {
                    actual: ValueKind::from_value(value),
                },
            })
        }
    }
}

// validation/adapters/lenient.rs
pub struct LenientValidator;

impl SchemaValidator for LenientValidator {
    fn is_assignable_from(&self, target: &ValueSchema, source: &ValueSchema) -> bool {
        // Coerces types, accepts more combinations
        match (target, source) {
            (ValueSchema::Dynamic, _) => true,
            (ValueSchema::String, ValueSchema::Number) => true, // Lenient: number → string
            (ValueSchema::Number, ValueSchema::Integer) => true,
            // ... lenient rules
            _ => target == source,
        }
    }

    fn validate(&self, schema: &ValueSchema, value: &JsonValue) -> Result<(), SchemaViolation> {
        // More permissive validation
    }

    fn validate_definition(&self, schema: &ValueSchema) -> Result<(), SchemaBuildError> {
        // More lenient definition checks
    }
}
```

### Serialization Adapters

Serde-based deserializers with validation integrated.

```rust
// serialization/serde_adapter.rs

use serde::{Deserialize, Serialize};
use crate::domain::{ValueSchema, HandlerMetadata};
use crate::validation::SchemaValidator;

/// Serde intermediate type (internal only)
#[derive(Deserialize)]
#[serde(tag = "type", content = "definition", rename_all = "snake_case")]
enum ValueSchemaWireFormat {
    Dynamic,
    Null,
    String,
    Array { items: Box<ValueSchemaWireFormat> },
    Object(ObjectSchemaWireFormat),
    // ... other variants
}

impl ValueSchemaWireFormat {
    /// Convert wire format to domain type with optional validation
    pub fn into_domain(self, validator: &dyn SchemaValidator) -> Result<ValueSchema, DeserializeError> {
        let domain = match self {
            Self::Dynamic => ValueSchema::Dynamic,
            Self::Array { items } => {
                let items = items.into_domain(validator)?;
                ValueSchema::Array {
                    items: Box::new(items),
                }
            }
            // ... other cases
        };

        // Validate definition after reconstruction
        validator
            .validate_definition(&domain)
            .map_err(DeserializeError::InvalidSchema)?;

        Ok(domain)
    }
}

/// Deserializer with built-in validation
pub struct ValidatingDeserializer<V: SchemaValidator> {
    validator: V,
}

impl<V: SchemaValidator> ValidatingDeserializer<V> {
    pub fn new(validator: V) -> Self {
        Self { validator }
    }

    pub fn deserialize_metadata(
        &self,
        yaml: &str,
    ) -> Result<HandlerMetadata, DeserializeError> {
        // Deserialize wire format
        let wire: SerializedMetadata = serde_saphyr::from_str(yaml)?;

        // Convert domain, validating schemas along the way
        Ok(HandlerMetadata {
            name: wire.name,
            input_schema: wire
                .input_schema
                .map(|w| w.into_domain(&self.validator))
                .transpose()?,
            output_schema: wire
                .output_schema
                .map(|w| w.into_domain(&self.validator))
                .transpose()?,
            // ... other fields
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeserializeError {
    #[error("YAML parse error: {0}")]
    ParseError(#[from] serde_saphyr::Error),
    #[error("invalid schema: {0}")]
    InvalidSchema(SchemaBuildError),
}
```

### Composition Root

Wire validators and serializers to domain.

```rust
// In crux-script lib.rs or main.rs

use crate::domain::*;
use crate::validation::{SchemaValidator, StrictValidator, LenientValidator};
use crate::serialization::ValidatingDeserializer;

/// Application configuration
pub struct MetadataContext {
    validator: Box<dyn SchemaValidator>,
    deserializer: Box<dyn MetadataDeserializer>,
}

impl MetadataContext {
    /// Create with strict validation (production)
    pub fn strict() -> Self {
        let validator = Box::new(StrictValidator);
        let deserializer = Box::new(ValidatingDeserializer::new(StrictValidator));
        Self {
            validator,
            deserializer,
        }
    }

    /// Create with lenient validation (for backward compatibility, migration)
    pub fn lenient() -> Self {
        let validator = Box::new(LenientValidator);
        let deserializer = Box::new(ValidatingDeserializer::new(LenientValidator));
        Self {
            validator,
            deserializer,
        }
    }

    /// Create with custom validator
    pub fn with_validator<V: SchemaValidator + 'static>(validator: V) -> Self {
        let deserializer = Box::new(ValidatingDeserializer::new(validator.clone()));
        Self {
            validator: Box::new(validator),
            deserializer,
        }
    }
}
```

---

## Migration Path

### Phase 1: Extract Domain Types (Non-Breaking)

1. Create `domain.rs` with pure types (no validation, no serde)
2. Keep old `metadata.rs` unchanged
3. Add `From` impls to convert between old and new types
4. No public API changes

### Phase 2: Create Validation Port (New Code)

1. Define `validation/mod.rs` with `SchemaValidator` trait
2. Implement `StrictValidator` (replica of current logic)
3. Create `validation/adapters/` for variants
4. Add validation tests against domain types

### Phase 3: Create Serialization Adapter (New Code)

1. Create `serialization/serde_adapter.rs` with wire format
2. Implement `ValidatingDeserializer`
3. Update `load()`, `load_file()` to use new deserializer
4. Test with existing YAML files

### Phase 4: Switch Public APIs (Breaking)

1. Update `load()` to return domain types instead of old metadata
2. Add `MetadataContext` to public API
3. Deprecate old validation methods
4. Update all call sites in `crux-cli`, `crux-agentic`, etc.

### Phase 5: Remove Old Code (Cleanup)

1. Delete old `metadata.rs`
2. Delete all old validation methods
3. Update docs

---

## Design Benefits

### SOLID Principles

| Principle | Benefit |
|-----------|---------|
| **SRP** | Domain has one reason to change (structure); validators have one reason (logic); serializers have one reason (format) |
| **OCP** | Add new validators without modifying domain or serializers |
| **LSP** | Any `SchemaValidator` can replace another |
| **ISP** | Validators only depend on methods they use (not full domain) |
| **DIP** | Domain doesn't depend on validators; validators depend on domain trait |

### Testability

- Unit test validators with mock schemas
- Unit test serialization with mock validators
- No circular dependencies
- Each layer tested in isolation

### Extensibility

- Add custom validators without modifying domain
- Add new serialization formats (protobuf, JSON Schema generation)
- Add custom error handling per validator
- Swap validators at runtime

### Maintainability

- Clear separation of concerns
- Small, focused modules
- Easier code review
- Easier debugging

---

## Code Organization

```
crates/crux-script/src/
├── metadata/
│   ├── mod.rs              # Re-exports
│   ├── domain.rs           # Pure types
│   ├── validation/
│   │   ├── mod.rs          # Port definition
│   │   └── adapters/
│   │       ├── strict.rs   # Strict validator
│   │       ├── lenient.rs  # Lenient validator
│   │       └── custom.rs   # Example: custom validator
│   └── serialization/
│       ├── mod.rs          # Deserializer trait
│       └── serde_adapter.rs # Serde implementation
├── lib.rs                  # Public API re-exports
└── (other crux-script modules)
```

---

## Example Usage

### Creating Metadata (Domain Layer)

```rust
use crux_script::metadata::domain::*;

let schema = ValueSchema::object(
    ObjectSchema::new()
        .required("name", ValueSchema::String)
        .optional("age", ValueSchema::Integer)
);

let handler = HandlerMetadata::new("my_handler")
    .input_schema(schema.clone())
    .output_schema(ValueSchema::String);

// No validation yet — it's just data
```

### Validating Metadata (With Validator)

```rust
use crux_script::metadata::validation::StrictValidator;

let validator = StrictValidator;

// Validate a schema definition
validator.validate_definition(&schema)?;

// Validate a value against the schema
let value = json!({ "name": "Alice", "age": 30 });
validator.validate(&schema, &value)?;

// Check assignability
assert!(validator.is_assignable_from(&schema, &another_schema));
```

### Deserializing with Validation

```rust
use crux_script::metadata::{MetadataContext, domain::HandlerMetadata};

let yaml = r#"
name: my_handler
input_schema:
  type: object
  definition:
    properties:
      name: { type: string }
output_schema:
  type: string
"#;

let context = MetadataContext::strict();
let handler: HandlerMetadata = context.deserialize_yaml(yaml)?;
```

---

## Open Questions

1. **Should validators be cloneable?** (For use in Arc<dyn Trait>)
   - Recommendation: Yes, use `Clone` + object-safe wrapper if needed

2. **Should deserializer validation be configurable per call?**
   - Recommendation: Yes, pass validator as parameter to deserialize method

3. **Should we support schema defaults/normalization?**
   - Recommendation: Add `MetadataNormalizer` as separate port if needed

4. **Backward compatibility: how lenient should lenient mode be?**
   - Recommendation: Document explicitly; tie to schema version

---

## References

- SOLID principles: https://en.wikipedia.org/wiki/SOLID
- Hexagonal architecture: https://alistair.cockburn.us/hexagonal-architecture/
- Ports and adapters: https://en.wikipedia.org/wiki/Ports_and_adapters_architecture
