/// Route-on-confidence: dispatch to different handlers based on a confidence score.
///
/// Shows:
/// - `CruxCtx::route_on_confidence(name, score, routes)` with `ConfidenceRoute` tuples
/// - `ConfidenceRange::exclusive` and `ConfidenceRange::inclusive` for gap-free coverage
/// - Ranges must be non-overlapping and collectively cover [0.0, 1.0]
/// - Matched route is recorded as a step; label appears in `step.name`
use cruxai::prelude::*;

async fn classify(ctx: &mut CruxCtx, confidence: f32) -> Result<String, CruxErr> {
    let routes: Vec<ConfidenceRoute<'_, String>> = vec![
        (
            ConfidenceRange::exclusive(0.0, 0.4),
            "low",
            Box::pin(async { Ok("Low confidence — escalate to human review.".to_string()) }),
        ),
        (
            ConfidenceRange::exclusive(0.4, 0.8),
            "medium",
            Box::pin(async { Ok("Medium confidence — log for spot-check.".to_string()) }),
        ),
        (
            ConfidenceRange::inclusive(0.8, 1.0),
            "high",
            Box::pin(async { Ok("High confidence — auto-approve.".to_string()) }),
        ),
    ];

    ctx.route_on_confidence("classify_confidence", confidence, routes)
        .await
}

#[tokio::main]
async fn main() {
    for &score in &[0.1_f32, 0.55, 0.92] {
        let mut ctx = CruxCtx::new("confidence_router");
        let action = classify(&mut ctx, score).await.unwrap();
        let crux = ctx.finalize(Ok(action.clone()));

        println!("Confidence {score:.2} → {action}");
        for step in crux.causal_chain() {
            println!("  step: {} ({:?})", step.name, step.status);
        }
        println!();
    }
}
