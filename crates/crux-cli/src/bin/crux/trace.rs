use crux_types::crux_value::Crux;
use serde_json::Value;

pub fn cmd_trace(
    path: &str,
    status: Option<&str>,
    kind: Option<&str>,
    min_confidence: Option<f32>,
    mermaid: bool,
) {
    let trace = load(path).unwrap_or_else(|error| fail(&error));
    if mermaid {
        println!("{}", trace.to_mermaid());
        return;
    }

    println!("Timeline: {} ({})", trace.agent, trace.id);
    for (index, step) in trace.steps.iter().enumerate().filter(|(_, step)| {
        status.is_none_or(|filter| format!("{:?}", step.status).eq_ignore_ascii_case(filter))
            && kind.is_none_or(|filter| format!("{:?}", step.kind).eq_ignore_ascii_case(filter))
            && min_confidence.is_none_or(|minimum| step.confidence >= minimum)
    }) {
        println!(
            "{index:>3} {:<10} {:<12} {:>6}ms {:>5.2} {}",
            format!("{:?}", step.status),
            format!("{:?}", step.kind),
            step.duration_ms,
            step.confidence,
            step.name
        );
    }
}

fn load(path: &str) -> Result<Crux<Value>, String> {
    let contents = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&contents).map_err(|error| error.to_string())
}

fn fail(message: &str) -> ! {
    eprintln!("trace explorer: {message}");
    std::process::exit(1)
}
