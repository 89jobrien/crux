/// Integration tests for route_on_confidence, pipe, and join_all.
use cruxx::prelude::*;

// -- route_on_confidence -------------------------------------------------------

#[cruxx::agent]
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
    let cruxx = classify_agent(0.2).await;
    assert_eq!(cruxx.value().unwrap(), "low");
    assert!(cruxx.steps.iter().any(|s| s.name == "classify::low"));
}

#[tokio::test]
async fn route_on_confidence_medium() {
    let cruxx = classify_agent(0.65).await;
    assert_eq!(cruxx.value().unwrap(), "medium");
    assert!(cruxx.steps.iter().any(|s| s.name == "classify::medium"));
}

#[tokio::test]
async fn route_on_confidence_high() {
    let cruxx = classify_agent(1.0).await;
    assert_eq!(cruxx.value().unwrap(), "high");
    assert!(cruxx.steps.iter().any(|s| s.name == "classify::high"));
}

// -- pipe ---------------------------------------------------------------------

type BoxFutStr =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, CruxErr>> + Send>>;
type BoxStage = Box<dyn FnOnce(String) -> BoxFutStr + Send>;

fn stage(f: impl FnOnce(String) -> BoxFutStr + Send + 'static) -> BoxStage {
    Box::new(f)
}

#[cruxx::agent]
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
    let cruxx = transform_agent("  hello  ".to_string()).await;
    assert_eq!(cruxx.value().unwrap(), "HELLO!");
    let names: Vec<_> = cruxx.steps.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"transform::upper"));
    assert!(names.contains(&"transform::trim"));
    assert!(names.contains(&"transform::exclaim"));
}

// -- join_all -----------------------------------------------------------------

#[cruxx::agent]
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
    let cruxx = fan_out_agent(()).await;
    let results = cruxx.value().unwrap();
    assert_eq!(results, &[10, 20, 30]);
    assert_eq!(cruxx.steps.len(), 3);
    assert!(cruxx.steps.iter().any(|s| s.name == "fetch::first"));
    assert!(cruxx.steps.iter().any(|s| s.name == "fetch::second"));
    assert!(cruxx.steps.iter().any(|s| s.name == "fetch::third"));
}

#[cruxx::agent]
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
    let cruxx = fan_out_failing(()).await;
    assert!(cruxx.value().is_err());
}
