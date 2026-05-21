use cruxx_script::{HandlerMetadata, HandlerRegistry, RiskLevel};
use serde_json::{Value, json};

pub fn register(registry: &mut HandlerRegistry) {
    registry.handler_value_with_metadata(
        HandlerMetadata::new("text::parse_vimgrep")
            .describe("Parse rg --vimgrep output into structured match records.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let text = input
                .get("text")
                .or_else(|| input.get("output"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Ok(json!({ "matches": parse_vimgrep(text) }))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("text::parse_jsonl")
            .describe(
                "Parse newline-delimited JSON into an array of values, skipping invalid lines.",
            )
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let text = input
                .get("text")
                .or_else(|| input.get("output"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Ok(json!({ "items": parse_jsonl(text) }))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("text::parse_frontmatter")
            .describe("Parse YAML frontmatter fenced by --- lines, returning frontmatter and body.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let text = input
                .get("text")
                .or_else(|| input.get("output"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let (frontmatter, body) = parse_frontmatter(text);
            Ok(json!({ "frontmatter": frontmatter, "body": body }))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("text::parse_diff")
            .describe("Parse unified diff output into structured file and hunk records.")
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let text = input
                .get("text")
                .or_else(|| input.get("output"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Ok(json!({ "files": parse_diff(text) }))
        },
    );

    registry.handler_value_with_metadata(
        HandlerMetadata::new("text::parse_branch_list")
            .describe(
                "Parse git branch output into a list of branch names with current-branch flag.",
            )
            .risk(RiskLevel::Low)
            .deterministic(true),
        |input: Value| async move {
            let text = input
                .get("text")
                .or_else(|| input.get("output"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Ok(json!({ "branches": parse_branch_list(text) }))
        },
    );
}

// ---------------------------------------------------------------------------
// Pure parsing functions (public for fuzz targets)
// ---------------------------------------------------------------------------

pub fn parse_vimgrep(input: &str) -> Vec<Value> {
    input
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let (file, rest) = line.split_once(':')?;
            let (line_no, rest) = rest.split_once(':')?;
            let (col, text) = rest.split_once(':')?;
            Some(json!({
                "file": file,
                "line": line_no.parse::<u64>().unwrap_or(0),
                "col": col.parse::<u64>().unwrap_or(0),
                "text": text.trim(),
            }))
        })
        .collect()
}

pub fn parse_jsonl(input: &str) -> Vec<Value> {
    input
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

pub fn parse_frontmatter(input: &str) -> (Value, String) {
    let trimmed = input.trim_start();
    if !trimmed.starts_with("---") {
        return (Value::Null, input.to_string());
    }

    let after_open = match trimmed.strip_prefix("---") {
        Some(rest) => rest.trim_start_matches('-'),
        None => return (Value::Null, input.to_string()),
    };
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);

    if let Some(close_pos) = find_closing_fence(after_open) {
        let yaml_block = &after_open[..close_pos];
        let body = &after_open[close_pos..];
        let body = body
            .strip_prefix("---")
            .unwrap_or(body)
            .trim_start_matches('-')
            .strip_prefix('\n')
            .unwrap_or(body);

        let fm: Value = serde_yaml::from_str(yaml_block).unwrap_or(Value::Null);
        (fm, body.to_string())
    } else {
        (Value::Null, input.to_string())
    }
}

fn find_closing_fence(input: &str) -> Option<usize> {
    let mut pos = 0;
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("---") && trimmed.chars().all(|c| c == '-') && !trimmed.is_empty() {
            return Some(pos);
        }
        pos += line.len() + 1;
    }
    None
}

pub fn parse_diff(input: &str) -> Vec<Value> {
    let mut files: Vec<Value> = Vec::new();
    let mut current_file: Option<String> = None;
    let mut hunks: Vec<Value> = Vec::new();
    let mut current_hunk: Option<HunkState> = None;

    for line in input.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk.to_value());
            }
            if let Some(file) = current_file.take() {
                files.push(json!({ "file": file, "hunks": hunks }));
                hunks = Vec::new();
            }
            current_file = Some(path.to_string());
        } else if line.starts_with("+++ ") {
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk.to_value());
            }
            if let Some(file) = current_file.take() {
                files.push(json!({ "file": file, "hunks": hunks }));
                hunks = Vec::new();
            }
            current_file = Some(line.strip_prefix("+++ ").unwrap_or(line).to_string());
        } else if line.starts_with("--- ") {
            // skip
        } else if line.starts_with("@@ ") {
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk.to_value());
            }
            let (old_start, new_start) = parse_hunk_header(line);
            current_hunk = Some(HunkState {
                old_start,
                new_start,
                lines: Vec::new(),
            });
        } else if line.starts_with("diff --git") {
            // ignore
        } else if let Some(ref mut hunk) = current_hunk {
            hunk.lines.push(line.to_string());
        }
    }

    if let Some(hunk) = current_hunk {
        hunks.push(hunk.to_value());
    }
    if let Some(file) = current_file {
        files.push(json!({ "file": file, "hunks": hunks }));
    }

    files
}

struct HunkState {
    old_start: u64,
    new_start: u64,
    lines: Vec<String>,
}

impl HunkState {
    fn to_value(&self) -> Value {
        json!({
            "old_start": self.old_start,
            "new_start": self.new_start,
            "lines": self.lines,
        })
    }
}

fn parse_hunk_header(line: &str) -> (u64, u64) {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let old_start = parts
        .get(1)
        .and_then(|s| s.strip_prefix('-'))
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let new_start = parts
        .get(2)
        .and_then(|s| s.strip_prefix('+'))
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (old_start, new_start)
}

pub fn parse_branch_list(input: &str) -> Vec<Value> {
    input
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let trimmed = line.trim();
            let (current, name) = if let Some(rest) = trimmed.strip_prefix("* ") {
                (true, rest.trim())
            } else {
                (false, trimmed)
            };
            json!({
                "name": name,
                "current": current,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_vimgrep_basic() {
        let input = "src/main.rs:10:5:TODO: fix this\nsrc/lib.rs:20:1:FIXME: broken\n";
        let result = parse_vimgrep(input);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["file"], "src/main.rs");
        assert_eq!(result[0]["line"], 10);
        assert_eq!(result[0]["col"], 5);
        assert_eq!(result[0]["text"], "TODO: fix this");
        assert_eq!(result[1]["file"], "src/lib.rs");
    }

    #[test]
    fn parse_vimgrep_empty() {
        assert_eq!(parse_vimgrep(""), Vec::<Value>::new());
    }

    #[test]
    fn parse_vimgrep_malformed_skipped() {
        let input = "no-colons-here\nfoo:1:2:ok\n";
        let result = parse_vimgrep(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["file"], "foo");
    }

    #[test]
    fn parse_jsonl_basic() {
        let input = r#"{"a":1}
{"b":2}
not json
{"c":3}
"#;
        let result = parse_jsonl(input);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0]["a"], 1);
        assert_eq!(result[2]["c"], 3);
    }

    #[test]
    fn parse_jsonl_empty() {
        assert_eq!(parse_jsonl(""), Vec::<Value>::new());
    }

    #[test]
    fn parse_frontmatter_basic() {
        let input = "---\ntitle: Test\nstatus: open\n---\nBody text here.";
        let (fm, body) = parse_frontmatter(input);
        assert_eq!(fm["title"], "Test");
        assert_eq!(fm["status"], "open");
        assert_eq!(body.trim(), "Body text here.");
    }

    #[test]
    fn parse_frontmatter_no_fence() {
        let input = "Just plain text.";
        let (fm, body) = parse_frontmatter(input);
        assert_eq!(fm, Value::Null);
        assert_eq!(body, "Just plain text.");
    }

    #[test]
    fn parse_frontmatter_empty() {
        let (fm, _) = parse_frontmatter("");
        assert_eq!(fm, Value::Null);
    }

    #[test]
    fn parse_diff_basic() {
        let input = "\
diff --git a/foo.rs b/foo.rs
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,4 @@
 line1
+added
 line2
 line3
";
        let result = parse_diff(input);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["file"], "foo.rs");
        let hunks = result[0]["hunks"].as_array().unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0]["old_start"], 1);
        assert_eq!(hunks[0]["new_start"], 1);
        let lines = hunks[0]["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn parse_diff_multiple_files() {
        let input = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,1 +1,1 @@
-old
+new
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -5,2 +5,3 @@
 ctx
+insert
 ctx
";
        let result = parse_diff(input);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["file"], "a.rs");
        assert_eq!(result[1]["file"], "b.rs");
    }

    #[test]
    fn parse_diff_empty() {
        assert_eq!(parse_diff(""), Vec::<Value>::new());
    }

    #[test]
    fn parse_branch_list_basic() {
        let input = "  feature/foo\n* main\n  develop\n";
        let result = parse_branch_list(input);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0]["name"], "feature/foo");
        assert!(!result[0]["current"].as_bool().unwrap());
        assert_eq!(result[1]["name"], "main");
        assert!(result[1]["current"].as_bool().unwrap());
    }

    #[test]
    fn parse_branch_list_empty() {
        assert_eq!(parse_branch_list(""), Vec::<Value>::new());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn jsonl_round_trip(items in prop::collection::vec(
            prop::collection::hash_map("[a-z]{1,4}", "[a-z0-9]{0,8}", 0..4),
            0..10,
        )) {
            let jsonl: String = items.iter()
                .map(|m| serde_json::to_string(&m).expect("serialize"))
                .collect::<Vec<_>>()
                .join("\n");
            let parsed = parse_jsonl(&jsonl);
            prop_assert_eq!(parsed.len(), items.len());
        }

        #[test]
        fn vimgrep_never_panics(input in ".*") {
            let _ = parse_vimgrep(&input);
        }

        #[test]
        fn frontmatter_never_panics(input in ".*") {
            let _ = parse_frontmatter(&input);
        }

        #[test]
        fn diff_never_panics(input in ".*") {
            let _ = parse_diff(&input);
        }

        #[test]
        fn branch_list_never_panics(input in ".*") {
            let _ = parse_branch_list(&input);
        }
    }
}
