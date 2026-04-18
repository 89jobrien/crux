/// End-to-end integration test: a multi-stage research pipeline.
///
/// Exercises all five combinators in a single coherent scenario:
///   1. `join_all`  — fan out to three simulated data sources in parallel
///   2. `pipe`      — clean and score each result sequentially
///   3. `speculate` — pick the highest-scoring cleaned result
///   4. `delegate`  — summarize the winner via a child agent
///   5. `route_on_confidence` — decide publish vs. escalate based on score
mod common;

use common::CounterAgent;
use cruxai::prelude::*;

// -- Types --------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SearchResult {
    source: String,
    text: String,
    score: f32,
}

// -- Pipeline agent -----------------------------------------------------------

#[cruxai::agent]
async fn research_pipeline(query: String) -> Crux<String> {
    // 1. Fan out: query three sources in parallel.
    let raw: Vec<String> = t
        .join_all(
            "fetch",
            vec![
                (
                    "web",
                    Box::pin({
                        let q = query.clone();
                        async move { Ok(format!("  Web result for {q}: highly relevant content.  ")) }
                    }),
                ),
                (
                    "docs",
                    Box::pin({
                        let q = query.clone();
                        async move { Ok(format!("Docs result for {q}: somewhat relevant.")) }
                    }),
                ),
                (
                    "code",
                    Box::pin({
                        let q = query.clone();
                        async move { Ok(format!("  Code result for {q}: exact match found.  ")) }
                    }),
                ),
            ],
        )
        .await?;

    // 2. Pipe each result through a cleaning + scoring stage.
    //    We process each source independently and collect scored results.
    let mut scored: Vec<SearchResult> = Vec::new();
    let sources = ["web", "docs", "code"];
    for (source, text) in sources.iter().zip(raw.into_iter()) {
        let cleaned: String = t
            .pipe(
                &format!("clean_{source}"),
                text,
                vec![
                    (
                        "trim",
                        Box::new(|s: String| -> BoxFut<String> {
                            Box::pin(async move { Ok(s.trim().to_string()) })
                        }),
                    ),
                    (
                        "normalize",
                        Box::new(|s: String| -> BoxFut<String> {
                            Box::pin(async move { Ok(s.to_lowercase()) })
                        }),
                    ),
                ],
            )
            .await?;
        let score = (cleaned.len() as f32 / 100.0).min(1.0);
        scored.push(SearchResult { source: source.to_string(), text: cleaned, score });
    }

    // 3. Speculate: pick the highest-scoring result.
    let best: SearchResult = t
        .speculate(
            "pick_best",
            scored
                .into_iter()
                .map(|r| {
                    let name = r.source.clone();
                    let fut: BoxFut<SearchResult> = Box::pin(async move { Ok(r) });
                    (name.leak() as &'static str, fut)
                })
                .collect(),
        )
        .pick_best_by(|r| r.score)
        .await?;

    let winning_text = best.text.clone();
    let winning_text2 = winning_text.clone();
    let winning_score = best.score;

    // 4. Delegate: count words in the winner via the shared CounterAgent.
    let word_count = t
        .delegate::<CounterAgent>("summarize", winning_text.clone())
        .run()
        .await?;

    // 5. Route on confidence: publish or escalate.
    let action: String = t
        .route_on_confidence(
            "decide",
            winning_score,
            vec![
                (
                    ConfidenceRange::exclusive(0.0, 0.5),
                    "escalate",
                    Box::pin(async move {
                        Ok(format!("Escalated for review: {winning_text} ({word_count} words)"))
                    }),
                ),
                (
                    ConfidenceRange::inclusive(0.5, 1.0),
                    "publish",
                    Box::pin(async move {
                        Ok(format!("Published: {winning_text2} ({word_count} words)"))
                    }),
                ),
            ],
        )
        .await?;

    Ok(action)
}

// -- Tests --------------------------------------------------------------------

#[tokio::test]
async fn pipeline_succeeds_and_produces_output() {
    let crux = research_pipeline("crux agentic DSL".to_string()).await;
    assert!(crux.value().is_ok());
    let output = crux.value().unwrap();
    assert!(output.contains("words"));
}

#[tokio::test]
async fn pipeline_records_fetch_steps() {
    let crux = research_pipeline("test query".to_string()).await;
    let fetch_steps: Vec<_> = crux
        .steps
        .iter()
        .filter(|s| s.name.starts_with("fetch::"))
        .collect();
    assert_eq!(fetch_steps.len(), 3);
    assert!(fetch_steps.iter().any(|s| s.name == "fetch::web"));
    assert!(fetch_steps.iter().any(|s| s.name == "fetch::docs"));
    assert!(fetch_steps.iter().any(|s| s.name == "fetch::code"));
}

#[tokio::test]
async fn pipeline_records_pipe_steps_per_source() {
    let crux = research_pipeline("test query".to_string()).await;
    for source in ["web", "docs", "code"] {
        assert!(
            crux.steps
                .iter()
                .any(|s| s.name == format!("clean_{source}::trim")),
            "missing clean_{source}::trim"
        );
        assert!(
            crux.steps
                .iter()
                .any(|s| s.name == format!("clean_{source}::normalize")),
            "missing clean_{source}::normalize"
        );
    }
}

#[tokio::test]
async fn pipeline_speculation_has_one_winner() {
    let crux = research_pipeline("test query".to_string()).await;
    let spec_steps: Vec<_> = crux
        .steps
        .iter()
        .filter(|s| s.name.starts_with("pick_best::"))
        .collect();
    assert_eq!(spec_steps.len(), 3);
    let winners: Vec<_> = spec_steps.iter().filter(|s| s.is_ok()).collect();
    assert_eq!(winners.len(), 1);
}

#[tokio::test]
async fn pipeline_has_delegation_child() {
    let crux = research_pipeline("test query".to_string()).await;
    assert_eq!(crux.children.len(), 1);
    assert_eq!(crux.children[0].agent, "counter");
}

#[tokio::test]
async fn pipeline_decide_step_recorded() {
    let crux = research_pipeline("test query".to_string()).await;
    let decide = crux
        .steps
        .iter()
        .find(|s| s.name.starts_with("decide::"));
    assert!(decide.is_some());
    assert!(decide.unwrap().is_ok());
}
