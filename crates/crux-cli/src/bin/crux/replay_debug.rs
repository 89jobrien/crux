use crux_schema::Crux;
use serde_json::Value;

pub fn cmd_replay_debug(path: &str, step: Option<usize>, compare: Option<&str>) {
    let trace = match load(path) {
        Ok(trace) => trace,
        Err(error) => fail(&error),
    };
    if let Some(index) = step {
        let Some(step) = trace.steps.get(index) else {
            fail(&format!("step {index} is out of range"));
        };
        println!("step {index}: {}", step.name);
        println!("status: {:?}", step.status);
        println!("kind: {:?}", step.kind);
        println!("origin: {:?}", step.origin);
        println!("input hash: {}", step.input_hash);
        println!("content hash: {:?}", step.content_hash);
        println!("output: {}", step.output.as_ref().unwrap_or(&Value::Null));
        if let Some(error) = &step.error {
            println!("error: {error}");
        }
    } else {
        for (index, step) in trace.steps.iter().enumerate() {
            println!(
                "{index:>3} {:?} {:?} {}",
                step.status, step.origin, step.name
            );
        }
    }

    if let Some(other) = compare {
        let other = load(other).unwrap_or_else(|error| fail(&error));
        match trace
            .steps
            .iter()
            .zip(&other.steps)
            .position(|(left, right)| {
                left.name != right.name
                    || left.input_hash != right.input_hash
                    || left.status != right.status
            }) {
            Some(index) => println!("first replay mismatch at step {index}"),
            None if trace.steps.len() == other.steps.len() => println!("traces match"),
            None => println!("trace lengths differ after common prefix"),
        }
    }
}

fn load(path: &str) -> Result<Crux<Value>, String> {
    let contents = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&contents).map_err(|error| error.to_string())
}

fn fail(message: &str) -> ! {
    eprintln!("replay debugger: {message}");
    std::process::exit(1)
}
