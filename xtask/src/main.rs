//! Self-updating xtask shim that delegates to the `taskit` binary.
//!
//! Usage: `cargo xtask <subcommand> [args...]`
//!
//! If `taskit` is not installed, this shim installs it automatically
//! via `cargo install taskit`.

mod publish;

use publish::{parse_publish_args, run_publish};
use std::process::{Command, exit};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(|s| s.as_str()) == Some("publish") {
        match parse_publish_args(&args[1..]) {
            Ok(publish_args) => {
                if let Err(e) = run_publish(publish_args) {
                    eprintln!("publish failed: {e}");
                    exit(1);
                }
            }
            Err(e) => {
                eprintln!("usage error: {e}");
                exit(2);
            }
        }
        return;
    }

    match Command::new("taskit").args(&args).status() {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("taskit not found, installing via cargo install...");
            let install = Command::new("cargo")
                .args(["install", "taskit"])
                .status()
                .expect("failed to run cargo install");
            if !install.success() {
                eprintln!("failed to install taskit");
                exit(1);
            }
            let status = Command::new("taskit")
                .args(&args)
                .status()
                .expect("failed to run taskit after install");
            exit(status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("failed to run taskit: {e}");
            exit(1);
        }
    }
}
