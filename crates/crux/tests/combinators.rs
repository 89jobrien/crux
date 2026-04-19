/// Integration tests for route_on_confidence, pipe, and join_all.
use cruxai::prelude::*;

// -- route_on_confidence -------------------------------------------------------

#[cruxai::agent]
async fn classify_agent(confidence: f32) -> Crux<String> {
    let label: String = x
        .route_on_confidence(
            "classify",
            confidence,
            vec![
                (
                    ConfidenceRange::exclusive(0.0, 0.5),
                    "low",
                    Box::pin(async { Ok("low".to_string()) })
                        as std::pin::Pin<
                            Box<dyn std::future::Future<Output = Result<String, CruxErr>> + Send>,
                        >,
                ),
                (
                    ConfidenceRange::exclusive(0.5, 0.8),
                    "medium",
                    Box::pin(async { Ok("medium".to_string()) }),
                ),
                (
                    ConfidenceRange::inclusive(0.8, 1.0),
                    "high",
                    Box::pin(async { Ok("high".to_string()) }),
                ),
            ],
        )
        .await?;
    Ok(label)
}

#[tokio::test]
async fn route_on_confidence_low() {
    let crux = classify_agent(0.2).await;
    assert_eq!(crux.value().unwrap(), "low");
    assert!(crux.steps.iter().any(|s| s.name == "classify::low"));
}

#[tokio::test]
async fn route_on_confidence_medium() {
    let crux = classify_agent(0.65).await;
    assert_eq!(crux.value().unwrap(), "medium");
    assert!(crux.steps.iter().any(|s| s.name == "classify::medium"));
}

#[tokio::test]
async fn route_on_confidence_high() {
    let crux = classify_agent(1.0).await;
    assert_eq!(crux.value().unwrap(), "high");
    assert!(crux.steps.iter().any(|s| s.name == "classify::high"));
}

// -- pipe ---------------------------------------------------------------------

type BoxFutStr =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, CruxErr>> + Send>>;
type BoxStage = Box<dyn FnOnce(String) -> BoxFutStr + Send>;

fn stage(f: impl FnOnce(String) -> BoxFutStr + Send + 'static) -> BoxStage {
    Box::new(f)
}

#[cruxai::agent]
async fn transform_agent(input: String) -> Crux<String> {
    let result: String = x
        .pipe(
            "transform",
            input,
            vec![
                (
                    "upper",
                    stage(|s| Box::pin(async move { Ok(s.to_uppercase()) })),
                ),
                (
                    "trim",
                    stage(|s| Box::pin(async move { Ok(s.trim().to_string()) })),
                ),
                (
                    "exclaim",
                    stage(|s| Box::pin(async move { Ok(format!("{s}!")) })),
                ),
            ],
        )
        .await?;
    Ok(result)
}

#[tokio::test]
async fn pipe_transforms_sequentially() {
    let crux = transform_agent("  hello  ".to_string()).await;
    assert_eq!(crux.value().unwrap(), "HELLO!");
    let names: Vec<_> = crux.steps.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"transform::upper"));
    assert!(names.contains(&"transform::trim"));
    assert!(names.contains(&"transform::exclaim"));
}

// -- join_all -----------------------------------------------------------------

#[cruxai::agent]
async fn fan_out_agent(_input: ()) -> Crux<Vec<i32>> {
    let results: Vec<i32> = x
        .join_all(
            "fetch",
            vec![
                ("first", Box::pin(async { Ok(10_i32) })),
                ("second", Box::pin(async { Ok(20_i32) })),
                ("third", Box::pin(async { Ok(30_i32) })),
            ],
        )
        .await?;
    Ok(results)
}

#[tokio::test]
async fn join_all_collects_all_arms() {
    let crux = fan_out_agent(()).await;
    let results = crux.value().unwrap();
    assert_eq!(results, &[10, 20, 30]);
    assert_eq!(crux.steps.len(), 3);
    assert!(crux.steps.iter().any(|s| s.name == "fetch::first"));
    assert!(crux.steps.iter().any(|s| s.name == "fetch::second"));
    assert!(crux.steps.iter().any(|s| s.name == "fetch::third"));
}

#[cruxai::agent]
async fn fan_out_failing(_input: ()) -> Crux<Vec<i32>> {
    let results: Vec<i32> = x
        .join_all(
            "fetch",
            vec![
                ("ok", Box::pin(async { Ok(1_i32) })),
                (
                    "bad",
                    Box::pin(async { Err(CruxErr::step_failed("bad", "network error")) }),
                ),
            ],
        )
        .await?;
    Ok(results)
}

#[tokio::test]
async fn join_all_propagates_error() {
    let crux = fan_out_failing(()).await;
    assert!(crux.value().is_err());
}
