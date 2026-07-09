#[allow(dead_code)]
pub(crate) struct PublishArgs {
    pub from: Option<String>,
}

#[allow(dead_code)]
pub(crate) fn parse_publish_args(args: &[String]) -> Result<PublishArgs, String> {
    let mut from = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--from" => {
                let val = iter
                    .next()
                    .ok_or_else(|| "--from requires a crate name".to_string())?;
                from = Some(val.clone());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(PublishArgs { from })
}

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
    fn parse_no_args_returns_none_from() {
        let args: Vec<String> = vec![];
        let result = parse_publish_args(&args).unwrap();
        assert_eq!(result.from, None);
    }

    #[test]
    fn parse_from_flag_captures_crate_name() {
        let args = vec!["--from".to_string(), "crux-runtime".to_string()];
        let result = parse_publish_args(&args).unwrap();
        assert_eq!(result.from.as_deref(), Some("crux-runtime"));
    }

    #[test]
    fn parse_from_flag_missing_value_returns_err() {
        let args = vec!["--from".to_string()];
        assert!(parse_publish_args(&args).is_err());
    }

    #[test]
    fn parse_unknown_flag_returns_err() {
        let args = vec!["--unknown".to_string()];
        assert!(parse_publish_args(&args).is_err());
    }

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
