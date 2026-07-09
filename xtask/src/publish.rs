use std::fmt;

pub(crate) struct CrateSpec {
    pub name: &'static str,
}

pub(crate) const PUBLISH_ORDER: &[CrateSpec] = &[
    CrateSpec { name: "crux-types" },
    CrateSpec { name: "crux-model" },
    CrateSpec {
        name: "crux-domain",
    },
    CrateSpec {
        name: "crux-macros",
    },
    CrateSpec {
        name: "crux-runtime",
    },
    CrateSpec {
        name: "crux-script",
    },
    CrateSpec { name: "crux-task" },
    CrateSpec {
        name: "crux-improve",
    },
    CrateSpec { name: "crux-baml" },
    CrateSpec {
        name: "crux-stdlib",
    },
    CrateSpec {
        name: "crux-plugin",
    },
    CrateSpec {
        name: "crux-agentic",
    },
    CrateSpec {
        name: "crux-planner",
    },
    CrateSpec { name: "crux" },
];

pub(crate) const POLL_RETRIES: u32 = 30;
pub(crate) const POLL_INTERVAL_SECS: u64 = 10;

#[derive(Debug)]
pub(crate) enum PublishError {
    CargoPublishFailed {
        crate_name: String,
        exit_code: i32,
    },
    IndexPollTimeout {
        crate_name: String,
        version: String,
    },
    HttpError {
        crate_name: String,
        source: Box<ureq::Error>,
    },
    VersionNotInWorkspace,
    UnknownFromCrate {
        name: String,
    },
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CargoPublishFailed {
                crate_name,
                exit_code,
            } => write!(
                f,
                "cargo publish -p {crate_name} failed with exit code {exit_code}"
            ),
            Self::IndexPollTimeout {
                crate_name,
                version,
            } => write!(
                f,
                "timed out waiting for {crate_name} {version} to appear on crates.io"
            ),
            Self::HttpError { crate_name, source } => {
                write!(f, "HTTP error polling index for {crate_name}: {source}")
            }
            Self::VersionNotInWorkspace => {
                write!(f, "could not read version from workspace Cargo.toml")
            }
            Self::UnknownFromCrate { name } => {
                write!(f, "--from crate '{name}' not found in publish order")
            }
        }
    }
}

impl std::error::Error for PublishError {}

pub(crate) fn workspace_version() -> Result<String, PublishError> {
    let manifest =
        std::fs::read_to_string("Cargo.toml").map_err(|_| PublishError::VersionNotInWorkspace)?;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("version")
            && let Some(val) = trimmed.split('"').nth(1)
        {
            return Ok(val.to_string());
        }
    }
    Err(PublishError::VersionNotInWorkspace)
}

pub(crate) struct PublishArgs {
    pub from: Option<String>,
}

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

pub(crate) fn run_publish(args: PublishArgs) -> Result<(), PublishError> {
    let version = workspace_version()?;
    eprintln!("publishing crux workspace v{version}");

    let crates = match args.from.as_deref() {
        None => PUBLISH_ORDER,
        Some(name) => {
            let pos = PUBLISH_ORDER
                .iter()
                .position(|c| c.name == name)
                .ok_or_else(|| PublishError::UnknownFromCrate {
                    name: name.to_string(),
                })?;
            &PUBLISH_ORDER[pos..]
        }
    };

    for spec in crates {
        eprintln!("publishing {} ...", spec.name);
        cargo_publish(spec.name)?;
        eprintln!("waiting for {} to be indexed ...", spec.name);
        wait_for_index(spec.name, &version)?;
    }

    eprintln!("all crates published successfully");
    Ok(())
}

pub(crate) fn cargo_publish(crate_name: &str) -> Result<(), PublishError> {
    let status = std::process::Command::new("cargo")
        .args(["publish", "-p", crate_name])
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn cargo: {e}"));
    if status.success() {
        Ok(())
    } else {
        Err(PublishError::CargoPublishFailed {
            crate_name: crate_name.to_string(),
            exit_code: status.code().unwrap_or(-1),
        })
    }
}

pub(crate) fn wait_for_index(crate_name: &str, version: &str) -> Result<(), PublishError> {
    let url = sparse_index_url(crate_name);
    for attempt in 1..=POLL_RETRIES {
        match ureq::get(&url).call() {
            Ok(response) => {
                let body = response.into_string().unwrap_or_default();
                if version_in_index_body(&body, version) {
                    eprintln!("  [{crate_name}] indexed after {attempt} poll(s)");
                    return Ok(());
                }
            }
            Err(e) => {
                return Err(PublishError::HttpError {
                    crate_name: crate_name.to_string(),
                    source: Box::new(e),
                });
            }
        }
        eprintln!(
            "  [{crate_name}] not yet indexed (attempt {attempt}/{POLL_RETRIES}), waiting {POLL_INTERVAL_SECS}s..."
        );
        std::thread::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS));
    }
    Err(PublishError::IndexPollTimeout {
        crate_name: crate_name.to_string(),
        version: version.to_string(),
    })
}

pub(crate) fn sparse_index_url(crate_name: &str) -> String {
    let c1 = &crate_name[..2];
    let c2 = &crate_name[2..4];
    format!("https://index.crates.io/{c1}/{c2}/{crate_name}")
}

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
    fn publish_order_contains_fourteen_crates() {
        assert_eq!(PUBLISH_ORDER.len(), 14);
    }

    #[test]
    fn publish_order_starts_with_leaves() {
        assert_eq!(PUBLISH_ORDER[0].name, "crux-types");
        assert_eq!(PUBLISH_ORDER[1].name, "crux-model");
    }

    #[test]
    fn publish_order_ends_with_facade() {
        assert_eq!(PUBLISH_ORDER[13].name, "crux");
    }

    #[test]
    fn workspace_version_parses_from_cargo_toml() {
        let version = workspace_version().unwrap();
        let parts: Vec<&str> = version.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()));
    }

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

    #[test]
    fn wait_for_index_would_succeed_if_version_present_on_first_poll() {
        let body = "{\"name\":\"crux\",\"vers\":\"0.3.1\",\"deps\":[],\"cksum\":\"x\"}";
        assert!(
            version_in_index_body(body, "0.3.1"),
            "first-poll success requires version_in_index_body to return true"
        );
    }

    fn crates_from(from: Option<&str>) -> Result<&'static [CrateSpec], PublishError> {
        match from {
            None => Ok(PUBLISH_ORDER),
            Some(name) => {
                let pos = PUBLISH_ORDER
                    .iter()
                    .position(|c| c.name == name)
                    .ok_or_else(|| PublishError::UnknownFromCrate {
                        name: name.to_string(),
                    })?;
                Ok(&PUBLISH_ORDER[pos..])
            }
        }
    }

    fn run_publish_dry(args: PublishArgs) -> Result<(), PublishError> {
        crates_from(args.from.as_deref())?;
        Ok(())
    }

    #[test]
    fn run_publish_rejects_unknown_from_crate() {
        let args = PublishArgs {
            from: Some("not-a-real-crate".to_string()),
        };
        let err = run_publish_dry(args).unwrap_err();
        assert!(matches!(err, PublishError::UnknownFromCrate { .. }));
    }

    #[test]
    fn run_publish_dry_from_crux_planner_skips_twelve_crates() {
        let args = PublishArgs {
            from: Some("crux-planner".to_string()),
        };
        let remaining = crates_from(args.from.as_deref()).unwrap();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].name, "crux-planner");
    }
}
