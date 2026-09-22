#![allow(dead_code)]

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StepDef {
    ForEach(ForEachNode),
    Plain { step: String },
}

#[derive(Debug, Deserialize)]
struct ForEachNode {
    for_each: String,
    items: String,
    steps: Vec<StepDef>,
    #[serde(default, rename = "as")]
    binding: Option<String>,
}

#[test]
fn latest_serde_saphyr_rejects_dedicated_binding_on_untagged_variant() {
    let yaml = r#"
for_each: doubles
items: "{{ input.numbers }}"
steps:
  - step: doubled
as: n
"#;

    let result = serde_saphyr::from_str::<StepDef>(yaml);
    assert!(
        result.is_err(),
        "remove the encoded binding workaround once this parses"
    );
}
