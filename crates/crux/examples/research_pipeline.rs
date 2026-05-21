/// Research pipeline: a full end-to-end crux agent.
///
/// Demonstrates all five combinators working together in a realistic scenario:
///
///   1. `join_all`            — fan out to three data sources concurrently
///   2. `pipe`                — clean and score each result
///   3. `speculate`           — pick the highest-scoring result
///   4. `delegate`            — summarize via a child agent
///   5. `route_on_confidence` — publish or escalate based on score
///
/// Run with: cargo run --example research_pipeline
use crux::prelude::*;

// -- Child agent: word counter ------------------------------------------------

struct WordCountAgent;

impl Agent for WordCountAgent {
    type Input = String;
    type Output = usize;

    fn name() -> &'static str {
        "word_count"
    }

    async fn run(ctx: &mut CruxCtx, input: Self::Input) -> Result<Self::Output, CruxErr> {
        ctx.step("count_words", || async move {
            Ok(input.split_whitespace().count())
        })
        .await
    }
}

// -- Scored result type -------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SearchResult {
    source: String,
    text: String,
    score: f32,
}

// -- Pipeline -----------------------------------------------------------------

async fn run_pipeline(query: &str) -> Crux<String> {
    let mut ctx = CruxCtx::new("research_pipeline");

    let result = pipeline(&mut ctx, query.to_string()).await;
    ctx.finalize(result)
}

async fn pipeline(ctx: &mut CruxCtx, query: String) -> Result<String, CruxErr> {
    // 1. Fan out: query three sources in parallel.
    let raw: Vec<String> = ctx
        .join_all(
            "fetch",
            vec![
                (
                    "web",
                    Box::pin({
                        let q = query.clone();
                        async move { Ok(format!("  Web: highly relevant content about {q}.  ")) }
                    }),
                ),
                (
                    "docs",
                    Box::pin({
                        let q = query.clone();
                        async move { Ok(format!("Docs: moderate overview of {q}.")) }
                    }),
                ),
                (
                    "code",
                    Box::pin({
                        let q = query.clone();
                        async move { Ok(format!("  Code: exact implementation match for {q}.  ")) }
                    }),
                ),
            ],
        )
        .await?;

    // 2. Pipe: clean and score each result.
    let sources = ["web", "docs", "code"];
    let mut scored: Vec<SearchResult> = Vec::new();

    for (source, text) in sources.iter().zip(raw.into_iter()) {
        let cleaned: String = ctx
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
        let score = (cleaned.len() as f32 / 80.0).min(1.0);
        scored.push(SearchResult {
            source: source.to_string(),
            text: cleaned,
            score,
        });
    }

    // 3. Speculate: pick the highest-scoring result.
    let best: SearchResult = ctx
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

    // 4. Delegate: count words in the winning result.
    let word_count = ctx
        .delegate::<WordCountAgent>("count_words", winning_text.clone())
        .with_budget(Budget::calls(5))
        .run()
        .await?;

    // 5. Route on confidence: publish or escalate.
    let action: String = ctx
        .route_on_confidence(
            "decide",
            winning_score,
            vec![
                (
                    ConfidenceRange::exclusive(0.0, 0.5),
                    "escalate",
                    Box::pin(async move {
                        Ok(format!(
                            "[ESCALATE] Low confidence ({winning_score:.2}). \
                             Needs review: \"{winning_text}\" ({word_count} words)"
                        ))
                    }),
                ),
                (
                    ConfidenceRange::inclusive(0.5, 1.0),
                    "publish",
                    Box::pin(async move {
                        Ok(format!(
                            "[PUBLISH] Confidence {winning_score:.2}. \
                             Result: \"{winning_text2}\" ({word_count} words)"
                        ))
                    }),
                ),
            ],
        )
        .await?;

    Ok(action)
}

// -- Main ---------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let query = "crux agentic DSL";
    let crux = run_pipeline(query).await;

    println!("Query:  {query}");
    println!("Output: {}", crux.value().unwrap());
    println!();
    println!(
        "Trace ({} steps, {} children):",
        crux.steps.len(),
        crux.children.len()
    );
    for step in crux.causal_chain() {
        println!("  [{:?}] {} — {:?}", step.kind, step.name, step.status);
    }
    println!();
    if let Some(child) = crux.children.first() {
        println!("Child agent: {}", child.agent);
        for step in child.causal_chain() {
            println!("  [{:?}] {} — {:?}", step.kind, step.name, step.status);
        }
    }
    println!();
    println!("Duration: {:?}ms", crux.duration_ms());
}
