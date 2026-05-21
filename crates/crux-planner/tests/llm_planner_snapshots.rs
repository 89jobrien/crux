//! Golden snapshot tests for LlmPlanner (Issue #20).
//!
//! Uses an `InMemoryGenerator` test double — no live API key required.

use crux_planner::generator::{InMemoryGenerator, LlmPlannerGeneric};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Build a planner wired to a canned response.
fn planner(response: &str) -> LlmPlannerGeneric<InMemoryGenerator> {
    let stub = InMemoryGenerator::new(response.to_string());
    LlmPlannerGeneric::new(stub)
}

// ── snapshot tests ───────────────────────────────────────────────────────────

#[test]
fn snapshot_read_and_extract_entities() {
    let canned = indoc::indoc! {"
        pipeline: extract_entities
        steps:
          - step: read
            handler: fs::read
            path: input.txt
          - step: extract
            handler: llm::extract
            function: ExtractEntities
            input: \"$step:read\"
          - step: write
            handler: json::write
            content: \"$step:extract\"
            path: output.json
    "};
    let yaml = planner(canned)
        .plan("Read a file and extract named entities")
        .unwrap();
    insta::assert_snapshot!("read_and_extract_entities", yaml);
}

#[test]
fn snapshot_git_review() {
    let canned = indoc::indoc! {"
        pipeline: git_review
        steps:
          - step: diff
            handler: git::diff
            base_ref: origin/main
          - step: analyze
            handler: llm::extract
            function: FindCodeIssues
            input: \"$step:diff\"
          - step: output
            handler: json::write
            content: \"$step:analyze\"
            path: review.json
    "};
    let yaml = planner(canned)
        .plan("Review a git commit and summarize changes")
        .unwrap();
    insta::assert_snapshot!("git_review", yaml);
}

#[test]
fn snapshot_csv_email_extraction() {
    let canned = indoc::indoc! {"
        pipeline: csv_email_extraction
        steps:
          - step: read
            handler: fs::read
            path: input.csv
          - step: parse
            handler: json::parse
            input: \"$step:read\"
          - step: extract_emails
            handler: llm::extract
            function: ExtractEmails
            input: \"$step:parse\"
          - step: write
            handler: json::write
            content: \"$step:extract_emails\"
            path: emails.json
    "};
    let yaml = planner(canned)
        .plan("Read CSV, parse rows, extract email addresses, write to JSON")
        .unwrap();
    insta::assert_snapshot!("csv_email_extraction", yaml);
}

#[test]
fn snapshot_summarize_document() {
    let canned = indoc::indoc! {"
        pipeline: summarize_document
        steps:
          - step: read
            handler: fs::read
            path: document.txt
          - step: summarize
            handler: llm::extract
            function: Summarize
            input: \"$step:read\"
            max_sentences: 3
          - step: output
            handler: json::write
            content: \"$step:summarize\"
            path: summary.json
    "};
    let yaml = planner(canned)
        .plan("Summarize a research paper and extract key concepts")
        .unwrap();
    insta::assert_snapshot!("summarize_document", yaml);
}

#[test]
fn snapshot_interactive_chat() {
    let canned = indoc::indoc! {"
        pipeline: interactive_chat
        steps:
          - step: read
            handler: fs::read
            path: document.txt
          - step: chat
            handler: llm::invoke
            function: Chat
            input: \"$step:read\"
    "};
    let yaml = planner(canned)
        .plan("Have a conversation with a crux agent about this document")
        .unwrap();
    insta::assert_snapshot!("interactive_chat", yaml);
}
