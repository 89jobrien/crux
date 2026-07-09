#[allow(dead_code)]
pub(crate) fn sparse_index_url(crate_name: &str) -> String {
    let c1 = &crate_name[..2];
    let c2 = &crate_name[2..4];
    format!("https://index.crates.io/{c1}/{c2}/{crate_name}")
}

#[allow(dead_code)]
pub(crate) fn version_in_index_body(body: &str, version: &str) -> bool {
    body.lines().any(|line| {
        if let Ok(obj) = serde_json::from_str::<serde_json::Value>(line) {
            obj.get("vers").and_then(|v| v.as_str()) == Some(version)
        } else {
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_index_url_four_char_prefix() {
        let url = sparse_index_url("crux-types");
        assert_eq!(url, "https://index.crates.io/cr/ux/crux-types");
    }

    #[test]
    fn sparse_index_url_facade_crate() {
        let url = sparse_index_url("crux");
        assert_eq!(url, "https://index.crates.io/cr/ux/crux");
    }

    #[test]
    fn version_in_index_body_found() {
        let body = r#"{"name":"crux-types","vers":"0.3.1","deps":[],"cksum":"abc"}
{"name":"crux-types","vers":"0.3.0","deps":[],"cksum":"def"}"#;
        assert!(version_in_index_body(body, "0.3.1"));
    }

    #[test]
    fn version_in_index_body_not_found() {
        let body = r#"{"name":"crux-types","vers":"0.3.0","deps":[],"cksum":"def"}"#;
        assert!(!version_in_index_body(body, "0.3.1"));
    }

    #[test]
    fn version_in_index_body_partial_match_not_counted() {
        let body = r#"{"name":"crux","vers":"0.3.10","deps":[],"cksum":"abc"}"#;
        assert!(!version_in_index_body(body, "0.3.1"));
    }
}
