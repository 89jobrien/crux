use std::fs;
use std::path::Path;

#[test]
fn schema_types_live_in_a_dedicated_crate() {
    let schema_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .join("crux-schema");
    let manifest = fs::read_to_string(schema_dir.join("Cargo.toml"))
        .expect("crux-schema must have its own manifest");

    assert!(manifest.contains("name = \"crux-schema\""));
    assert!(!manifest.contains("tokio"));
    assert!(!manifest.contains("reqwest"));
    assert!(schema_dir.join("src/crux_value.rs").is_file());
}
