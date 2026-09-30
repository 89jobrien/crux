use std::{collections::BTreeMap, path::PathBuf, str::FromStr as _};

use clap::Subcommand;
use crux_improve::RegressionPolicy;
use crux_regression::{FileRegressionStore, RegressionCaseId, RegressionHarness, TraceDigest};
use crux_schema::Crux;
use serde_json::Value;

#[derive(Debug, Subcommand)]
pub enum RegressCommand {
    /// Store an immutable raw trace and print its digest
    Ingest {
        trace: PathBuf,
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Set or replace the active baseline for a case
    Promote {
        case: String,
        digest: String,
        #[arg(long)]
        expected_current: Option<String>,
        #[arg(long = "label")]
        labels: Vec<String>,
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Evaluate a candidate trace against the active baseline
    Evaluate {
        case: String,
        candidate: PathBuf,
        #[arg(long)]
        policy: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// Compare candidate structure without persisting a report
    Diff {
        case: String,
        candidate: PathBuf,
        #[arg(long)]
        policy: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        store: Option<PathBuf>,
    },
    /// List persisted evaluation reports for a case
    History {
        case: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        store: Option<PathBuf>,
    },
}

pub fn cmd_regress(command: RegressCommand) {
    match run(command) {
        Ok(true) => {}
        Ok(false) => std::process::exit(2),
        Err(error) => {
            eprintln!("regression: {error}");
            std::process::exit(1);
        }
    }
}

fn run(command: RegressCommand) -> Result<bool, String> {
    match command {
        RegressCommand::Ingest { trace, store } => {
            let harness = open_harness(store)?;
            let trace = load_json(&trace)?;
            let digest = harness.ingest(&trace).map_err(|error| error.to_string())?;
            println!("{digest}");
            Ok(true)
        }
        RegressCommand::Promote {
            case,
            digest,
            expected_current,
            labels,
            store,
        } => {
            let harness = open_harness(store)?;
            let case = RegressionCaseId::new(case).map_err(|error| error.to_string())?;
            let digest = TraceDigest::from_str(&digest).map_err(|error| error.to_string())?;
            let expected = expected_current
                .as_deref()
                .map(TraceDigest::from_str)
                .transpose()
                .map_err(|error| error.to_string())?;
            let baseline = harness
                .promote(case, digest, expected, parse_labels(&labels)?)
                .map_err(|error| error.to_string())?;
            println!("promoted {} to {}", baseline.case, baseline.trace);
            Ok(true)
        }
        RegressCommand::Evaluate {
            case,
            candidate,
            policy,
            json,
            store,
        } => {
            let harness = open_harness(store)?;
            let case = RegressionCaseId::new(case).map_err(|error| error.to_string())?;
            let candidate: Crux<Value> = load_json(&candidate)?;
            let policy: RegressionPolicy = load_json(&policy)?;
            let report = harness
                .evaluate(&case, &candidate, &policy)
                .map_err(|error| error.to_string())?;
            if json {
                print_json(&report)?;
            } else {
                println!(
                    "{} {}: {}",
                    if report.evaluation.passed {
                        "PASS"
                    } else {
                        "FAIL"
                    },
                    report.case,
                    report.id
                );
                for failure in &report.evaluation.failures {
                    println!("  - {failure}");
                }
            }
            Ok(report.evaluation.passed)
        }
        RegressCommand::Diff {
            case,
            candidate,
            policy,
            json,
            store,
        } => {
            let harness = open_harness(store)?;
            let case = RegressionCaseId::new(case).map_err(|error| error.to_string())?;
            let candidate: Crux<Value> = load_json(&candidate)?;
            let policy: RegressionPolicy = load_json(&policy)?;
            let diff = harness
                .diff(&case, &candidate, &policy.structure)
                .map_err(|error| error.to_string())?;
            if json {
                print_json(&diff)?;
            } else {
                println!(
                    "{}: {} structural change(s)",
                    if diff.matches { "MATCH" } else { "DIFF" },
                    diff.step_diffs.len()
                );
            }
            Ok(diff.matches)
        }
        RegressCommand::History { case, json, store } => {
            let harness = open_harness(store)?;
            let case = RegressionCaseId::new(case).map_err(|error| error.to_string())?;
            let reports = harness.history(&case).map_err(|error| error.to_string())?;
            if json {
                print_json(&reports)?;
            } else if reports.is_empty() {
                println!("no regression reports for {case}");
            } else {
                for report in reports {
                    println!(
                        "{} {} {}",
                        report.id,
                        if report.evaluation.passed {
                            "PASS"
                        } else {
                            "FAIL"
                        },
                        report.created_at
                    );
                }
            }
            Ok(true)
        }
    }
}

fn open_harness(store: Option<PathBuf>) -> Result<RegressionHarness<FileRegressionStore>, String> {
    let root = match store {
        Some(root) => root,
        None => dirs::home_dir()
            .ok_or_else(|| "home directory is unavailable; pass --store".to_string())?
            .join(".crux/regressions"),
    };
    FileRegressionStore::open(root)
        .map(RegressionHarness::new)
        .map_err(|error| error.to_string())
}

fn load_json<T: serde::de::DeserializeOwned>(path: &PathBuf) -> Result<T, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn parse_labels(labels: &[String]) -> Result<BTreeMap<String, String>, String> {
    labels
        .iter()
        .map(|label| {
            let (key, value) = label
                .split_once('=')
                .ok_or_else(|| format!("invalid label '{label}'; expected KEY=VALUE"))?;
            if key.is_empty() {
                return Err(format!("invalid label '{label}'; key must not be empty"));
            }
            Ok((key.to_string(), value.to_string()))
        })
        .collect()
}
