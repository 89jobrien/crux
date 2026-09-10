use crux_runtime::prelude::*;
use crux_script::schema::{DisplayOutput, PipelineDisplayDef};
use serde_json::Value;

fn display_title<'a>(crux: &'a Crux<Value>, display: Option<&'a PipelineDisplayDef>) -> &'a str {
    display
        .and_then(|metadata| metadata.title.as_deref())
        .unwrap_or(&crux.agent)
}

fn display_step_name<'a>(name: &'a str, display: Option<&'a PipelineDisplayDef>) -> &'a str {
    display
        .and_then(|metadata| metadata.steps.get(name))
        .map(String::as_str)
        .unwrap_or(name)
}

fn format_duration(duration: std::time::Duration) -> String {
    if duration.as_secs() >= 1 {
        format!("{:.2}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn successful_shell_stdout(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    let is_shell_result = object.contains_key("exit_code")
        && object.contains_key("stdout")
        && object.contains_key("stderr");
    if !is_shell_result || object.get("exit_code")?.as_i64()? != 0 {
        return None;
    }
    object.get("stdout")?.as_str()
}

fn should_render_output(value: &Value, display: Option<&PipelineDisplayDef>) -> bool {
    match display.map_or(DisplayOutput::Auto, |metadata| metadata.output) {
        DisplayOutput::Auto => {
            successful_shell_stdout(value).is_none_or(|stdout| !stdout.is_empty())
        }
        DisplayOutput::Always => true,
        DisplayOutput::Never => false,
    }
}

fn append_output(out: &mut String, value: &Value) {
    out.push_str("\nOutput:\n");
    if let Some(stdout) = successful_shell_stdout(value) {
        out.push_str(stdout);
        if !stdout.ends_with('\n') {
            out.push('\n');
        }
    } else {
        let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
        out.push_str(&pretty);
        out.push('\n');
    }
}

fn append_error(out: &mut String, error: &CruxErr) {
    out.push_str("\nFailure:\n");
    for line in error.to_string().lines() {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
}

/// Render concise human-facing pipeline output for default or explicit summary mode.
pub fn render_summary(
    crux: &Crux<Value>,
    elapsed: std::time::Duration,
    display: Option<&PipelineDisplayDef>,
) -> String {
    let mut out = String::new();
    let status = if crux.value().is_ok() { "PASS" } else { "FAIL" };
    let title = display_title(crux, display);
    out.push_str(&format!(
        "{title}  {status}  {}\n\n",
        format_duration(elapsed)
    ));

    for step in &crux.steps {
        let icon = match step.status {
            StepStatus::Ok => "✓",
            StepStatus::Err => "✗",
            StepStatus::Rejected => "·",
            StepStatus::Skipped => "-",
        };
        let name = display_step_name(&step.name, display);
        let duration = format_duration(std::time::Duration::from_millis(step.duration_ms));
        out.push_str(&format!("  {icon} {name:<42} {duration:>8}\n"));
    }

    let passed = crux
        .steps
        .iter()
        .filter(|step| step.status == StepStatus::Ok)
        .count();
    out.push_str(&format!("\n{passed}/{} checks passed\n", crux.steps.len()));

    match crux.value() {
        Ok(value) if should_render_output(value, display) => append_output(&mut out, value),
        Ok(_) => {}
        Err(error) => append_error(&mut out, error),
    }

    out
}

/// Render the full trace envelope (pipeline info, per-step status, timing, output) as text.
pub fn render_trace(
    crux: &Crux<Value>,
    elapsed: std::time::Duration,
    display: Option<&PipelineDisplayDef>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("Pipeline: {}\n", display_title(crux, display)));
    out.push_str(&format!(
        "Status:   {}\n",
        if crux.value().is_ok() { "OK" } else { "FAILED" }
    ));
    out.push_str(&format!(
        "Duration: {:.1}ms\n",
        elapsed.as_secs_f64() * 1000.0
    ));
    out.push_str(&format!("Steps:    {}\n\n", crux.steps.len()));

    out.push_str("Trace:\n");
    for (i, step) in crux.steps.iter().enumerate() {
        let status = match step.status {
            StepStatus::Ok => "OK",
            StepStatus::Err => "ERR",
            StepStatus::Rejected => "REJ",
            StepStatus::Skipped => "SKIP",
        };
        let kind = match step.kind {
            StepKind::Plain => "",
            StepKind::Delegation => " [delegate]",
            StepKind::Branch => " [branch]",
            StepKind::Speculation => " [speculate]",
        };
        let name = display_step_name(&step.name, display);
        out.push_str(&format!(
            "  {:>2}. [{:>4}] {}{} ({}ms)\n",
            i + 1,
            status,
            name,
            kind,
            step.duration_ms
        ));
    }

    if let Ok(value) = crux.value()
        && should_render_output(value, display)
    {
        append_output(&mut out, value);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn step(name: &str, status: StepStatus, duration_ms: u64) -> Step {
        Step {
            name: name.to_string(),
            kind: StepKind::Plain,
            status,
            confidence: 1.0,
            started_at: chrono::Utc::now(),
            duration_ms,
            input_hash: 0,
            content_hash: None,
            output: None,
            error: None,
            attempt: 0,
            events: vec![],
            metadata: HashMap::new(),
            findings: vec![],
        }
    }

    fn crux(value: Result<Value, CruxErr>, steps: Vec<Step>) -> Crux<Value> {
        Crux {
            id: CruxId::new(),
            agent: "renderer".to_string(),
            value,
            steps,
            children: vec![],
            started_at: chrono::Utc::now(),
            finished_at: None,
        }
    }

    fn display(output: DisplayOutput) -> PipelineDisplayDef {
        PipelineDisplayDef {
            output,
            ..PipelineDisplayDef::default()
        }
    }

    #[test]
    fn summary_renderer_covers_display_modes_and_absent_metadata() {
        let shell = json!({"exit_code": 0, "stdout": "ok", "stderr": ""});
        let semantic = json!({"answer": 42});

        assert!(
            render_summary(
                &crux(Ok(shell.clone()), vec![]),
                std::time::Duration::ZERO,
                Some(&display(DisplayOutput::Auto)),
            )
            .contains("Output:\nok\n")
        );
        assert!(
            render_summary(
                &crux(Ok(shell), vec![]),
                std::time::Duration::ZERO,
                Some(&display(DisplayOutput::Always)),
            )
            .contains("Output:")
        );
        assert!(
            !render_summary(
                &crux(Ok(semantic.clone()), vec![]),
                std::time::Duration::ZERO,
                Some(&display(DisplayOutput::Never)),
            )
            .contains("Output:")
        );
        assert!(
            render_summary(&crux(Ok(semantic), vec![]), std::time::Duration::ZERO, None,)
                .contains("Output:")
        );
    }

    #[test]
    fn verbose_renderer_humanizes_shell_output_and_honors_display_modes() {
        let shell = json!({
            "exit_code": 0,
            "stdout": "Compiling crux\nFinished test profile\n",
            "stderr": "warning: build noise\n"
        });

        for mode in [DisplayOutput::Auto, DisplayOutput::Always] {
            let rendered = render_trace(
                &crux(Ok(shell.clone()), vec![]),
                std::time::Duration::ZERO,
                Some(&display(mode)),
            );
            assert!(
                rendered.contains("Output:\nCompiling crux\nFinished test profile\n"),
                "{rendered}"
            );
            assert!(!rendered.contains("exit_code"), "{rendered}");
            assert!(!rendered.contains("warning: build noise"), "{rendered}");
            assert!(!rendered.contains(r"\n"), "{rendered}");
        }

        let hidden = render_trace(
            &crux(Ok(shell), vec![]),
            std::time::Duration::ZERO,
            Some(&display(DisplayOutput::Never)),
        );
        assert!(!hidden.contains("Output:"), "{hidden}");
    }

    #[test]
    fn verbose_renderer_keeps_semantic_json_pretty() {
        let rendered = render_trace(
            &crux(Ok(json!({"answer": 42})), vec![]),
            std::time::Duration::ZERO,
            Some(&display(DisplayOutput::Auto)),
        );
        assert!(
            rendered.contains(
                "Output:
{
  \"answer\": 42
}"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn summary_renderer_covers_failure_rows_and_second_durations() {
        let rendered = render_summary(
            &crux(
                Err(CruxErr::step_failed("failed", "boom")),
                vec![
                    step("failed", StepStatus::Err, 1_250),
                    step("rejected", StepStatus::Rejected, 2_000),
                    step("skipped", StepStatus::Skipped, 0),
                ],
            ),
            std::time::Duration::from_millis(2_500),
            None,
        );

        assert!(rendered.contains("renderer  FAIL  2.50s"), "{rendered}");
        assert!(rendered.contains("✗ failed"), "{rendered}");
        assert!(rendered.contains("· rejected"), "{rendered}");
        assert!(rendered.contains("- skipped"), "{rendered}");
        assert!(rendered.contains("1.25s"), "{rendered}");
        assert!(rendered.contains("2.00s"), "{rendered}");
        assert!(rendered.contains("0/3 checks passed"), "{rendered}");
        assert!(rendered.contains("Failure:"), "{rendered}");
        assert!(
            rendered.contains("step 'failed' failed: boom"),
            "{rendered}"
        );
    }
}
