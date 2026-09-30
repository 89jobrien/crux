#!/usr/bin/env rust-script
//! ```cargo
//! [package]
//! edition = "2024"
//!
//! [dependencies]
//! anyhow = "1"
//! serde = { version = "1", features = ["derive"] }
//! serde_json = "1"
//! tiny_http = "0.12"
//! wait-timeout = "0.2"
//! ```

use std::env;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use wait_timeout::ChildExt;

const DASHBOARD_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>CRUX // FAULT &amp; RESUME</title>
  <style>
    :root {
      --bg: #000;
      --raised: #0a0a0a;
      --fg: #e0e0e0;
      --muted: #777b76;
      --amber: #f5a623;
      --green: #b7f34a;
      --red: #ff4d42;
      --border: #252825;
      --mono: "IBM Plex Mono", "Cascadia Code", "SFMono-Regular", monospace;
      --sans: "IBM Plex Sans", "Avenir Next", sans-serif;
    }
    * { box-sizing: border-box; }
    html { color-scheme: dark; }
    body {
      margin: 0;
      min-height: 100vh;
      color: var(--fg);
      font-family: var(--sans);
      background-color: var(--bg);
      background-image:
        linear-gradient(rgba(44, 48, 44, .16) 1px, transparent 1px),
        linear-gradient(90deg, rgba(44, 48, 44, .16) 1px, transparent 1px),
        radial-gradient(circle at 84% 6%, rgba(245, 166, 35, .08), transparent 32%);
      background-size: 28px 28px, 28px 28px, 100% 100%;
    }
    body::after {
      content: "";
      position: fixed;
      inset: 0;
      z-index: 20;
      pointer-events: none;
      opacity: .18;
      background: repeating-linear-gradient(0deg, transparent 0 3px, rgba(255,255,255,.025) 3px 4px);
    }
    .mono, .metric-value, .status, .eyebrow, .step-state { font-family: var(--mono); font-variant-numeric: tabular-nums; }
    .signal-bar { height: 5px; background: linear-gradient(90deg, var(--amber), #875608 42%, transparent 82%); box-shadow: 0 0 18px rgba(245,166,35,.42); }
    .shell { width: min(1440px, calc(100% - 40px)); margin: auto; padding: 22px 0 48px; }
    .topline { display: flex; align-items: center; justify-content: space-between; padding: 9px 0 18px; border-bottom: 1px solid var(--border); color: var(--muted); font: 11px/1.2 var(--mono); letter-spacing: .12em; text-transform: uppercase; }
    .brand, .eyebrow { color: var(--amber); }
    .connection { display: inline-flex; align-items: center; gap: 8px; }
    .connection-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--green); box-shadow: 0 0 10px rgba(183,243,74,.78); }
    .masthead { display: grid; grid-template-columns: minmax(0,1fr) auto; gap: 24px; align-items: end; padding: 38px 0 30px; }
    .eyebrow { margin: 0 0 12px; font: 700 11px/1 var(--mono); letter-spacing: .18em; text-transform: uppercase; }
    h1 { margin: 0; font: 600 clamp(35px,5.5vw,76px)/.98 var(--mono); letter-spacing: -.06em; }
    h1 span { color: var(--muted); font-weight: 300; }
    .run-state { min-width: 220px; padding: 16px 18px; border: 1px solid var(--border); background: var(--raised); box-shadow: 0 0 24px rgba(245,166,35,.09); }
    .run-state-label, .metric-label { color: var(--muted); font: 10px/1.2 var(--mono); letter-spacing: .14em; text-transform: uppercase; }
    .run-state-value { margin-top: 9px; color: var(--amber); font: 700 18px/1 var(--mono); text-transform: uppercase; }
    .run-state[data-phase="faulting"] .run-state-value, .run-state[data-phase="faulted"] .run-state-value, .run-state[data-phase="failed"] .run-state-value { color: var(--red); }
    .run-state[data-phase="complete"] .run-state-value { color: var(--green); }
    .metrics { display: grid; grid-template-columns: repeat(4,minmax(0,1fr)); border: 1px solid var(--border); background: var(--raised); }
    .metric { min-height: 96px; padding: 18px; border-right: 1px solid var(--border); }
    .metric:last-child { border: 0; }
    .metric-value { margin-top: 10px; font-size: 28px; line-height: 1; }
    .success { color: var(--green); }
    .panel { margin-top: 18px; border: 1px solid var(--border); background: rgba(10,10,10,.94); }
    .panel-head { display: flex; align-items: center; justify-content: space-between; min-height: 42px; padding: 0 16px; border-bottom: 1px solid var(--border); color: var(--muted); font: 10px/1 var(--mono); letter-spacing: .12em; text-transform: uppercase; }
    .execution-rail { display: grid; grid-template-columns: minmax(0,1fr) 52px minmax(0,1fr) 52px minmax(0,1fr); padding: 18px; }
    .connector { position: relative; min-height: 138px; }
    .connector::before { content: ""; position: absolute; top: 50%; left: 8px; right: 8px; height: 1px; background: var(--border); }
    .connector::after { content: ">"; position: absolute; top: calc(50% - 8px); right: 3px; color: var(--muted); font: 12px/1 var(--mono); }
    .stage { position: relative; overflow: hidden; min-height: 138px; padding: 17px; border: 1px solid var(--border); background: #070807; }
    .stage::after { content: ""; position: absolute; inset: 0; pointer-events: none; border-left: 2px solid transparent; }
    .stage.is-live { border-color: #5e410d; background: #0d0b06; }
    .stage.is-live::after { border-left-color: var(--amber); }
    .stage.is-failed { border-color: #5a1d19; background: #100706; }
    .stage.is-failed::after { border-left-color: var(--red); }
    .stage.is-ok, .stage.is-replayed { border-color: #334519; background: #090c06; }
    .stage.is-ok::after, .stage.is-replayed::after { border-left-color: var(--green); }
    .stage.is-replayed::before { content: ""; position: absolute; inset: 0; transform: translateX(-105%); background: linear-gradient(90deg,transparent,rgba(183,243,74,.18),transparent); animation: replay-sweep .75s cubic-bezier(.65,0,.35,1) forwards; }
    @keyframes replay-sweep { to { transform: translateX(105%); } }
    .stage-top { position: relative; z-index: 1; display: flex; justify-content: space-between; gap: 12px; }
    .stage-id { color: var(--muted); font: 10px/1 var(--mono); letter-spacing: .14em; }
    .step-state { color: var(--muted); font-size: 9px; letter-spacing: .08em; text-align: right; }
    .stage.is-live .step-state { color: var(--amber); }
    .stage.is-failed .step-state { color: var(--red); }
    .stage.is-ok .step-state, .stage.is-replayed .step-state { color: var(--green); }
    .stage h2 { position: relative; z-index: 1; margin: 30px 0 7px; font: 600 18px/1.1 var(--mono); }
    .endpoint { position: relative; z-index: 1; color: var(--muted); font: 11px/1.3 var(--mono); }
    .fault-card { display: none; grid-template-columns: 120px 1fr auto; gap: 18px; align-items: center; margin-top: 18px; padding: 16px 18px; border: 1px solid #5a1d19; background: #100706; color: #ffaca6; }
    .fault-card.is-visible { display: grid; }
    .fault-code { color: var(--red); font: 700 11px/1 var(--mono); letter-spacing: .1em; }
    .fault-message { font: 12px/1.45 var(--mono); }
    .fault-time { color: var(--muted); font: 11px/1 var(--mono); }
    .controls { display: flex; flex-wrap: wrap; gap: 10px; margin-top: 18px; padding: 14px; border: 1px solid var(--border); background: var(--raised); }
    .control-button { min-height: 38px; padding: 0 16px; border: 1px solid #554016; color: var(--amber); background: #0c0a05; font: 700 10px/1 var(--mono); letter-spacing: .08em; text-transform: uppercase; cursor: pointer; }
    .control-button:hover:not(:disabled), .control-button:focus-visible { border-color: var(--amber); box-shadow: 0 0 16px rgba(245,166,35,.16); outline: none; }
    .control-button.primary { color: #000; background: var(--amber); }
    .control-button.replay { color: var(--green); border-color: #40551e; }
    .control-button:disabled { opacity: .3; cursor: not-allowed; }
    .control-error { flex: 1 1 100%; min-height: 14px; color: var(--red); font: 10px/1.4 var(--mono); }
    .traces { display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); gap: 18px; margin-top: 18px; }
    .trace-panel { border: 1px solid var(--border); background: var(--raised); }
    .trace-panel.failed { border-top: 2px solid var(--red); }
    .trace-panel.resumed { border-top: 2px solid var(--green); }
    .trace-body { padding: 10px 16px 14px; }
    .trace-row { width: 100%; display: grid; grid-template-columns: 26px minmax(0,1fr) minmax(90px,auto) 54px; gap: 10px; align-items: center; min-height: 40px; padding: 0; border: 0; border-bottom: 1px solid #191b19; color: var(--fg); background: transparent; font: 11px/1 var(--mono); text-align: left; cursor: pointer; }
    .trace-row:hover, .trace-row:focus-visible, .trace-row.is-selected { background: #11130d; outline: none; }
    .trace-row:last-child { border: 0; }
    .trace-index, .trace-origin, .trace-duration { color: var(--muted); }
    .trace-origin, .trace-duration { text-align: right; }
    .trace-row.ok .trace-origin { color: var(--green); }
    .trace-row.err .trace-origin { color: var(--red); }
    .trace-panel.is-pending { opacity: .42; }
    .trace-empty { padding: 18px 0; color: var(--muted); font: 11px/1.4 var(--mono); }
    .detail-panel { margin-top: 18px; border: 1px solid var(--border); background: #050505; }
    .detail-grid { display: grid; grid-template-columns: 180px minmax(0,1fr); min-height: 170px; }
    .detail-meta { padding: 16px; border-right: 1px solid var(--border); color: var(--muted); font: 10px/1.8 var(--mono); }
    .detail-meta strong { display: block; color: var(--amber); font-size: 13px; }
    .detail-json { margin: 0; padding: 16px; overflow: auto; color: #c5d7b0; font: 11px/1.55 var(--mono); white-space: pre-wrap; word-break: break-word; }
    .state-strip { display: grid; grid-template-columns: auto 1fr; gap: 16px; margin-top: 18px; padding: 12px 16px; border: 1px solid var(--border); background: #050505; color: var(--muted); font: 10px/1.5 var(--mono); }
    .state-strip strong { color: var(--amber); font-weight: 500; }
    #raw-state { margin: 0; white-space: pre-wrap; word-break: break-word; }
    @media (max-width: 760px) {
      .shell { width: min(100% - 24px,640px); padding-top: 10px; }
      .session { display: none; }
      .masthead, .traces { grid-template-columns: 1fr; }
      .run-state { min-width: 0; }
      .metrics { grid-template-columns: repeat(2,1fr); }
      .metric:nth-child(2) { border-right: 0; }
      .execution-rail { grid-template-columns: 1fr; gap: 8px; }
      .connector { min-height: 24px; }
      .connector::before { top: 2px; bottom: 2px; left: 50%; right: auto; width: 1px; height: auto; }
      .connector::after { top: auto; bottom: -1px; right: calc(50% - 4px); transform: rotate(90deg); }
      .fault-card, .state-strip { grid-template-columns: 1fr; }
      .trace-row { grid-template-columns: 22px minmax(0,1fr); padding: 9px 0; }
      .trace-origin, .trace-duration { grid-column: 2; text-align: left; }
      .detail-grid { grid-template-columns: 1fr; }
      .detail-meta { border-right: 0; border-bottom: 1px solid var(--border); }
    }
    @media (prefers-reduced-motion: reduce) { .stage.is-replayed::before { animation: none; transform: none; opacity: .08; } }
  </style>
</head>
<body>
  <div class="signal-bar"></div>
  <main class="shell">
    <header class="topline">
      <div><span class="brand">CRUX</span> // LOCAL TRACE CONSOLE</div>
      <div class="connection" id="connection-status" aria-live="polite"><span class="connection-dot" id="connection-dot"></span><span id="connection-label">POLLING 100MS</span></div>
      <div class="session">SESSION // FAULT_RESUME</div>
    </header>
    <section class="masthead">
      <div><p class="eyebrow">Replay observability / deterministic recovery</p><h1>CRUX <span>//</span> FAULT &amp; RESUME</h1></div>
      <div class="run-state" id="run-state" data-phase="idle" aria-live="polite"><div class="run-state-label">Current run state</div><div class="run-state-value" id="run-state-value">Awaiting execution</div></div>
    </section>
    <section class="metrics" aria-label="Execution metrics">
      <div class="metric"><div class="metric-label">Pipeline HTTP calls</div><div class="metric-value" id="metric-requests">0</div></div>
      <div class="metric"><div class="metric-label">Cache hits</div><div class="metric-value success" id="metric-cache">0</div></div>
      <div class="metric"><div class="metric-label">Replayed duration</div><div class="metric-value success" id="metric-duration">--</div></div>
      <div class="metric"><div class="metric-label">Pipeline result</div><div class="metric-value" id="metric-result">IDLE</div></div>
    </section>
    <section class="controls" aria-label="Execution controls">
      <button class="control-button" id="control-next" type="button">Next Step</button>
      <button class="control-button replay" id="control-replay" type="button">Replay Failed Step</button>
      <button class="control-button primary" id="control-run-all" type="button">Run All</button>
      <button class="control-button" id="control-reset" type="button">Reset</button>
      <div class="control-error" id="control-error" role="status" aria-live="polite"></div>
    </section>
    <section class="panel">
      <div class="panel-head"><span>Execution rail</span><span id="rail-caption">Waiting for Step A</span></div>
      <div class="execution-rail">
        <article class="stage" id="stage-context"><div class="stage-top"><span class="stage-id">STEP A</span><span class="step-state" id="state-context">QUEUED</span></div><h2>fetch_context</h2><div class="endpoint">GET /context</div></article>
        <div class="connector" aria-hidden="true"></div>
        <article class="stage" id="stage-evaluate"><div class="stage-top"><span class="stage-id">STEP B</span><span class="step-state" id="state-evaluate">QUEUED</span></div><h2>evaluate_output</h2><div class="endpoint">POST /evaluate</div></article>
        <div class="connector" aria-hidden="true"></div>
        <article class="stage" id="stage-finalize"><div class="stage-top"><span class="stage-id">STEP C</span><span class="step-state" id="state-finalize">QUEUED</span></div><h2>finalize</h2><div class="endpoint">POST /finalize</div></article>
      </div>
    </section>
    <section class="fault-card" id="fault-card" role="status"><div class="fault-code">CRUX::STEP_FAILED</div><div class="fault-message">HTTP finalize exceeded its 100ms request budget. Trace persisted; completed outputs remain replayable.</div><div class="fault-time">STEP C // TIMEOUT</div></section>
    <section class="traces" aria-label="Trace comparison">
      <article class="trace-panel failed is-pending" id="failed-trace"><div class="panel-head"><span>Failed trace</span><span id="failed-status">PENDING</span></div><div class="trace-body" id="failed-trace-rows"><div class="trace-empty">No failed trace captured.</div></div></article>
      <article class="trace-panel resumed is-pending" id="current-trace"><div class="panel-head"><span>Current trace</span><span id="current-status">PENDING</span></div><div class="trace-body" id="current-trace-rows"><div class="trace-empty">Advance a step to begin tracing.</div></div></article>
    </section>
    <section class="detail-panel" aria-live="polite">
      <div class="panel-head"><span>Selected step detail</span><span id="detail-kind">NO SELECTION</span></div>
      <div class="detail-grid"><div class="detail-meta" id="detail-meta">Select any trace row.</div><pre class="detail-json" id="detail-json">{}</pre></div>
    </section>
    <section class="state-strip"><strong>LIVE STATE</strong><pre id="raw-state">{"phase":"idle","context":0,"evaluate":0,"finalize":0}</pre></section>
  </main>
  <script>
    const $ = (id) => document.getElementById(id);
    let lastPhase = "idle";
    let selectedKey = null;
    let currentTraceSignature = "";
    let failedTraceSignature = "";

    function markStage(name, className, label) {
      $("stage-" + name).className = "stage" + (className ? " " + className : "");
      $("state-" + name).textContent = label;
    }

    function showDetail(step, source, index) {
      selectedKey = source + ":" + index;
      document.querySelectorAll(".trace-row").forEach((row) => {
        row.classList.toggle("is-selected", row.dataset.key === selectedKey);
      });
      $("detail-kind").textContent = source.toUpperCase() + " TRACE";
      $("detail-meta").replaceChildren();
      const title = document.createElement("strong");
      title.textContent = step.name;
      $("detail-meta").append(
        title,
        document.createTextNode("status: " + step.status),
        document.createElement("br"),
        document.createTextNode("origin: " + step.origin),
        document.createElement("br"),
        document.createTextNode("duration: " + step.duration_ms + "ms"),
        document.createElement("br"),
        document.createTextNode("confidence: " + Number(step.confidence).toFixed(2))
      );
      $("detail-json").textContent = JSON.stringify(
        step.error ? { error: step.error } : { output: step.output },
        null,
        2
      );
    }

    function renderTrace(containerId, steps, source) {
      const container = $(containerId);
      container.replaceChildren();
      if (!steps.length) {
        const empty = document.createElement("div");
        empty.className = "trace-empty";
        empty.textContent = source === "failed" ? "No failed trace captured." : "Advance a step to begin tracing.";
        container.append(empty);
        return;
      }
      steps.forEach((step, index) => {
        const row = document.createElement("button");
        row.type = "button";
        row.className = "trace-row " + (step.status === "ok" ? "ok" : "err");
        row.dataset.key = source + ":" + index;
        if (row.dataset.key === selectedKey) row.classList.add("is-selected");
        const position = document.createElement("span");
        position.className = "trace-index";
        position.textContent = String.fromCharCode(65 + index);
        const name = document.createElement("span");
        name.textContent = step.name;
        const origin = document.createElement("span");
        origin.className = "trace-origin";
        origin.textContent = step.origin === "replayed" ? "CACHED / REPLAYED" : step.origin.toUpperCase();
        const duration = document.createElement("span");
        duration.className = "trace-duration";
        duration.textContent = step.duration_ms + "ms";
        row.append(position, name, origin, duration);
        row.addEventListener("click", () => showDetail(step, source, index));
        container.append(row);
      });
    }

    function renderStages(state) {
      const steps = new Map(state.current_trace.map((step) => [step.name, step]));
      for (const [shortName, traceName] of [["context", "fetch_context"], ["evaluate", "evaluate_output"], ["finalize", "finalize"]]) {
        const step = steps.get(traceName);
        if (step) {
          const replayed = step.origin === "replayed";
          const ok = step.status === "ok";
          markStage(
            shortName,
            replayed ? "is-replayed" : (ok ? "is-ok" : "is-failed"),
            replayed ? "CACHED / REPLAYED" : (ok ? "LIVE / OK" : "LIVE / ERR")
          );
        } else if (state.active_boundary === traceName) {
          markStage(shortName, "is-live", "LIVE / RUNNING");
        } else {
          markStage(shortName, "", "QUEUED");
        }
      }
    }

    function render(state) {
      const complete = state.phase === "complete";
      const faultVisible = state.phase === "faulting" || state.phase === "faulted";
      const labels = {
        idle: "Awaiting execution",
        ready: "Boundary complete",
        running_step: "Running next step",
        faulting: "Fault injected",
        faulted: "Trace halted",
        replaying: "Replaying trace",
        complete: "Resume complete",
        failed: "Command failed"
      };
      const cacheHits = state.current_trace.filter((step) => step.origin === "replayed").length;
      $("run-state").dataset.phase = state.phase;
      $("run-state-value").textContent = labels[state.phase] || state.phase;
      $("metric-requests").textContent = String(state.context + state.evaluate + state.finalize);
      $("metric-cache").textContent = String(cacheHits);
      $("metric-duration").textContent = cacheHits ? "0ms" : "--";
      $("metric-result").textContent = complete ? "OK" : (faultVisible ? "ERR" : state.phase.toUpperCase());
      $("metric-result").style.color = complete ? "var(--green)" : (faultVisible ? "var(--red)" : "var(--fg)");
      $("fault-card").classList.toggle("is-visible", faultVisible);
      $("failed-trace").classList.toggle("is-pending", state.failed_trace.length === 0);
      $("current-trace").classList.toggle("is-pending", state.current_trace.length === 0);
      $("failed-status").textContent = state.failed_trace.length ? "ERR / PERSISTED" : "PENDING";
      $("current-status").textContent = complete ? "OK / COMPLETE" : state.current_trace.length + " STEP(S)";
      $("raw-state").textContent = JSON.stringify(state);
      $("control-next").disabled = !state.can_next;
      $("control-replay").disabled = !state.can_replay;
      $("control-run-all").disabled = !state.can_run_all;
      $("control-reset").disabled = !state.can_reset;
      $("control-error").textContent = state.command_error || "";
      $("rail-caption").textContent = complete
        ? cacheHits + " cache hits // 1 live resume"
        : (state.active_boundary ? "Executing // " + state.active_boundary : "Boundary " + Math.min(state.next_step + 1, 3) + " / 3");
      renderStages(state);
      const nextFailedSignature = JSON.stringify(state.failed_trace);
      if (nextFailedSignature !== failedTraceSignature) {
        failedTraceSignature = nextFailedSignature;
        renderTrace("failed-trace-rows", state.failed_trace, "failed");
      }
      const nextCurrentSignature = JSON.stringify(state.current_trace);
      if (nextCurrentSignature !== currentTraceSignature) {
        currentTraceSignature = nextCurrentSignature;
        renderTrace("current-trace-rows", state.current_trace, "current");
        if (state.current_trace.length) {
          showDetail(state.current_trace.at(-1), "current", state.current_trace.length - 1);
        } else {
          selectedKey = null;
          $("detail-kind").textContent = "NO SELECTION";
          $("detail-meta").textContent = "Select any trace row.";
          $("detail-json").textContent = "{}";
        }
      }
      lastPhase = state.phase;
    }

    async function control(action) {
      try {
        const response = await fetch("/control/" + action, { method: "POST" });
        const body = await response.json();
        if (!response.ok) throw new Error(body.error || "control request failed");
        $("control-error").textContent = "";
      } catch (error) {
        $("control-error").textContent = error.message;
      }
    }

    $("control-next").addEventListener("click", () => control("next"));
    $("control-replay").addEventListener("click", () => control("replay"));
    $("control-run-all").addEventListener("click", () => control("run-all"));
    $("control-reset").addEventListener("click", () => control("reset"));

    async function poll() {
      try {
        const response = await fetch("/state", { cache: "no-store" });
        if (!response.ok) throw new Error("state endpoint returned " + response.status);
        render(await response.json());
        $("connection-dot").style.background = "var(--green)";
        $("connection-label").textContent = "POLLING 100MS";
      } catch (error) {
        $("rail-caption").textContent = "Telemetry reconnecting // " + lastPhase;
        $("connection-dot").style.background = "var(--red)";
        $("connection-label").textContent = "RECONNECTING";
      } finally {
        window.setTimeout(poll, 100);
      }
    }
    poll();
  </script>
</body>
</html>
"##;

const BOUNDARIES: [&str; 3] = ["fetch_context", "evaluate_output", "finalize"];
const MAX_BODY_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
enum DemoPhase {
    #[default]
    Idle,
    Ready,
    RunningStep,
    Faulting,
    Faulted,
    Replaying,
    Complete,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TraceStepView {
    name: String,
    status: String,
    origin: String,
    duration_ms: u64,
    confidence: f32,
    output: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct StoredTrace {
    steps: Vec<TraceStepView>,
}

#[derive(Clone, Default, Serialize)]
struct DemoState {
    phase: DemoPhase,
    context: u64,
    evaluate: u64,
    finalize: u64,
    finalize_in_flight: bool,
    busy: bool,
    next_step: usize,
    active_boundary: Option<String>,
    current_trace: Vec<TraceStepView>,
    failed_trace: Vec<TraceStepView>,
    command_error: Option<String>,
    can_next: bool,
    can_replay: bool,
    can_run_all: bool,
    can_reset: bool,
    #[serde(skip)]
    current_trace_path: Option<PathBuf>,
    #[serde(skip)]
    failed_trace_path: Option<PathBuf>,
}

#[derive(Clone)]
struct Config {
    address_file: PathBuf,
    state_file: PathBuf,
    crux_bin: PathBuf,
    pipeline: PathBuf,
    input: PathBuf,
    work_dir: PathBuf,
}

#[derive(Clone, Copy)]
enum ControlAction {
    Reset,
    Next,
    Replay,
    RunAll,
}

fn main() -> Result<()> {
    let config = parse_args()?;
    validate_config(&config)?;
    let server = Server::http("127.0.0.1:0")
        .map_err(|error| anyhow::anyhow!("failed to bind demo server: {error}"))?;
    let address = server.server_addr().to_string();
    atomic_write(&config.address_file, &format!("{address}\n"))?;
    atomic_write(
        &config.input,
        &serde_json::to_string_pretty(&serde_json::json!({
            "base_url": format!("http://{address}"),
            "finalize_timeout_ms": 100
        }))?,
    )?;

    let state = Arc::new(Mutex::new(DemoState::default()));
    update_state(&state, &config.state_file, |_| {})?;
    let (control_tx, control_rx) = mpsc::sync_channel(1);
    let worker_state = Arc::clone(&state);
    let worker_config = config.clone();
    thread::spawn(move || control_worker(control_rx, worker_state, worker_config));

    let finalize_busy = Arc::new(AtomicBool::new(false));
    for request in server.incoming_requests() {
        if request.url().split('?').next() == Some("/finalize") {
            if finalize_busy.swap(true, Ordering::AcqRel) {
                let _ = respond_json(
                    request,
                    StatusCode(429),
                    &serde_json::json!({"error": "finalize request already running"}),
                );
                continue;
            }
            let state = Arc::clone(&state);
            let config = config.clone();
            let control_tx = control_tx.clone();
            let finalize_busy = Arc::clone(&finalize_busy);
            thread::spawn(move || {
                if let Err(error) = handle_request(request, &state, &config, &control_tx) {
                    eprintln!("fault-resume server: {error:#}");
                }
                finalize_busy.store(false, Ordering::Release);
            });
        } else if let Err(error) = handle_request(request, &state, &config, &control_tx) {
            eprintln!("fault-resume server: {error:#}");
        }
    }

    Ok(())
}

fn parse_args() -> Result<Config> {
    let mut address_file = None;
    let mut state_file = None;
    let mut crux_bin = None;
    let mut pipeline = None;
    let mut input = None;
    let mut work_dir = None;
    let mut args = env::args().skip(1);

    while let Some(flag) = args.next() {
        let value = args
            .next()
            .with_context(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--address-file" => address_file = Some(PathBuf::from(value)),
            "--state-file" => state_file = Some(PathBuf::from(value)),
            "--crux-bin" => crux_bin = Some(PathBuf::from(value)),
            "--pipeline" => pipeline = Some(PathBuf::from(value)),
            "--input" => input = Some(PathBuf::from(value)),
            "--work-dir" => work_dir = Some(PathBuf::from(value)),
            _ => bail!("unknown argument: {flag}"),
        }
    }

    Ok(Config {
        address_file: address_file.context("--address-file is required")?,
        state_file: state_file.context("--state-file is required")?,
        crux_bin: crux_bin.context("--crux-bin is required")?,
        pipeline: pipeline.context("--pipeline is required")?,
        input: input.context("--input is required")?,
        work_dir: work_dir.context("--work-dir is required")?,
    })
}

fn validate_config(config: &Config) -> Result<()> {
    for (label, path) in [
        ("crux binary", &config.crux_bin),
        ("pipeline", &config.pipeline),
        ("work directory", &config.work_dir),
    ] {
        if !path.exists() {
            bail!("{label} does not exist: {}", path.display());
        }
    }
    Ok(())
}

fn handle_request(
    mut request: Request,
    state: &Arc<Mutex<DemoState>>,
    config: &Config,
    control_tx: &SyncSender<ControlAction>,
) -> Result<()> {
    let url = request.url().to_owned();
    let path = url.split('?').next().unwrap_or(&url);

    let expected_method = match path {
        "/" | "/state" | "/context" => Some(Method::Get),
        "/control/reset"
        | "/control/next"
        | "/control/replay"
        | "/control/run-all"
        | "/evaluate"
        | "/finalize" => Some(Method::Post),
        _ => None,
    };
    if let Some(expected) = expected_method
        && request.method() != &expected
    {
        return respond_json(
            request,
            StatusCode(405),
            &serde_json::json!({"error": "method not allowed"}),
        );
    }

    if path == "/" {
        return respond_html(request, DASHBOARD_HTML);
    }
    if path == "/state" {
        let snapshot = snapshot_state(state)?;
        return respond_json(request, StatusCode(200), &snapshot);
    }
    if request
        .body_length()
        .is_some_and(|length| length as u64 > MAX_BODY_BYTES)
    {
        return respond_json(
            request,
            StatusCode(413),
            &serde_json::json!({"error": "request body exceeds 64 KiB"}),
        );
    }
    let mut body = String::new();
    request
        .as_reader()
        .take(MAX_BODY_BYTES + 1)
        .read_to_string(&mut body)
        .context("failed to read request body")?;
    if body.len() as u64 > MAX_BODY_BYTES {
        return respond_json(
            request,
            StatusCode(413),
            &serde_json::json!({"error": "request body exceeds 64 KiB"}),
        );
    }

    if let Some(action) = control_action(path) {
        return queue_control(request, state, &config.state_file, control_tx, action);
    }

    let (status, response_body) = match path {
        "/context" => {
            let current = update_state(state, &config.state_file, |value| value.context += 1)?;
            (
                StatusCode(200),
                serde_json::json!({
                    "request_count": current.context,
                    "context": "customer requested a deterministic retry"
                }),
            )
        }
        "/evaluate" => {
            let current = update_state(state, &config.state_file, |value| value.evaluate += 1)?;
            (
                StatusCode(200),
                serde_json::json!({
                    "request_count": current.evaluate,
                    "score": 0.94,
                    "received_bytes": body.len()
                }),
            )
        }
        "/finalize" => {
            let current = update_state(state, &config.state_file, |value| {
                value.finalize += 1;
                value.finalize_in_flight = true;
                if value.finalize == 1 {
                    value.phase = DemoPhase::Faulting;
                }
            })?;
            let delay_ms = if current.finalize == 1 { 1000 } else { 0 };
            thread::sleep(Duration::from_millis(delay_ms));
            (
                StatusCode(200),
                serde_json::json!({
                    "request_count": current.finalize,
                    "status": "complete",
                    "received_bytes": body.len()
                }),
            )
        }
        _ => (StatusCode(404), serde_json::json!({"error": "not found"})),
    };

    // A faulting client intentionally disconnects before the delayed response is written.
    let _ = respond_json(request, status, &response_body);
    if path == "/finalize" {
        update_state(state, &config.state_file, |value| {
            value.finalize_in_flight = false;
        })?;
    }
    Ok(())
}

fn control_action(path: &str) -> Option<ControlAction> {
    match path {
        "/control/reset" => Some(ControlAction::Reset),
        "/control/next" => Some(ControlAction::Next),
        "/control/replay" => Some(ControlAction::Replay),
        "/control/run-all" => Some(ControlAction::RunAll),
        _ => None,
    }
}

fn queue_control(
    request: Request,
    state: &Arc<Mutex<DemoState>>,
    state_file: &Path,
    control_tx: &SyncSender<ControlAction>,
    action: ControlAction,
) -> Result<()> {
    {
        let snapshot = snapshot_state(state)?;
        if snapshot.busy || !action_allowed(&snapshot, action) {
            return respond_json(
                request,
                StatusCode(409),
                &serde_json::json!({"error": "control action is not available in the current state"}),
            );
        }
    }
    update_state(state, state_file, |value| value.busy = true)?;
    match control_tx.try_send(action) {
        Ok(()) => respond_json(
            request,
            StatusCode(202),
            &serde_json::json!({"accepted": true}),
        ),
        Err(TrySendError::Full(_)) => {
            update_state(state, state_file, |value| value.busy = false)?;
            respond_json(
                request,
                StatusCode(409),
                &serde_json::json!({"error": "control queue is full"}),
            )
        }
        Err(TrySendError::Disconnected(_)) => {
            update_state(state, state_file, |value| value.busy = false)?;
            respond_json(
                request,
                StatusCode(503),
                &serde_json::json!({"error": "control worker is unavailable"}),
            )
        }
    }
}

fn action_allowed(state: &DemoState, action: ControlAction) -> bool {
    match action {
        ControlAction::Reset => state.can_reset,
        ControlAction::Next => state.can_next,
        ControlAction::Replay => state.can_replay,
        ControlAction::RunAll => state.can_run_all,
    }
}

fn control_worker(
    receiver: mpsc::Receiver<ControlAction>,
    state: Arc<Mutex<DemoState>>,
    config: Config,
) {
    while let Ok(action) = receiver.recv() {
        let outcome = match action {
            ControlAction::Reset => reset_demo(&state, &config),
            ControlAction::Next => run_next(&state, &config),
            ControlAction::Replay => run_replay(&state, &config),
            ControlAction::RunAll => run_all(&state, &config),
        };
        let error = outcome.err().map(|error| format!("{error:#}"));
        if let Err(update_error) = update_state(&state, &config.state_file, |value| {
            value.busy = false;
            if let Some(error) = error {
                value.phase = DemoPhase::Failed;
                value.command_error = Some(error);
            }
        }) {
            eprintln!("fault-resume worker: {update_error:#}");
        }
    }
}

fn reset_demo(state: &Arc<Mutex<DemoState>>, config: &Config) -> Result<()> {
    for name in ["step-1.json", "step-2.json", "failed.json", "resumed.json"] {
        let path = config.work_dir.join(name);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }
    update_state(state, &config.state_file, |value| {
        *value = DemoState {
            busy: true,
            ..DemoState::default()
        };
    })?;
    Ok(())
}

fn run_next(state: &Arc<Mutex<DemoState>>, config: &Config) -> Result<()> {
    let snapshot = snapshot_state(state)?;
    let Some(boundary) = BOUNDARIES.get(snapshot.next_step) else {
        bail!("all top-level steps have already run");
    };
    let output_path = if snapshot.next_step == 2 {
        config.work_dir.join("failed.json")
    } else {
        config
            .work_dir
            .join(format!("step-{}.json", snapshot.next_step + 1))
    };
    update_state(state, &config.state_file, |value| {
        value.phase = DemoPhase::RunningStep;
        value.active_boundary = Some((*boundary).to_string());
        value.command_error = None;
    })?;

    let execution = execute_crux(
        config,
        Some(boundary),
        snapshot.current_trace_path.as_deref(),
        &output_path,
    )?;
    let trace = read_trace(&output_path)?;
    update_state(state, &config.state_file, |value| {
        value.current_trace = trace.steps.clone();
        value.current_trace_path = Some(output_path.clone());
        value.active_boundary = None;
        if execution.status.success() {
            value.next_step += 1;
            value.phase = if value.next_step == BOUNDARIES.len() {
                DemoPhase::Complete
            } else {
                DemoPhase::Ready
            };
        } else {
            value.phase = DemoPhase::Faulted;
            value.failed_trace = trace.steps;
            value.failed_trace_path = Some(output_path);
            value.command_error = nonempty_stderr(&execution.stderr);
        }
    })?;
    Ok(())
}

fn run_replay(state: &Arc<Mutex<DemoState>>, config: &Config) -> Result<()> {
    for _ in 0..80 {
        if !snapshot_state(state)?.finalize_in_flight {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    if snapshot_state(state)?.finalize_in_flight {
        bail!("timed out waiting for the failed finalize request to stop");
    }
    let snapshot = snapshot_state(state)?;
    let replay_path = snapshot
        .failed_trace_path
        .as_deref()
        .context("no failed trace is available to replay")?;
    let output_path = config.work_dir.join("resumed.json");
    update_state(state, &config.state_file, |value| {
        value.phase = DemoPhase::Replaying;
        value.active_boundary = Some("finalize".to_string());
        value.command_error = None;
    })?;
    let execution = execute_crux(config, None, Some(replay_path), &output_path)?;
    let trace = read_trace(&output_path)?;
    update_state(state, &config.state_file, |value| {
        value.current_trace = trace.steps;
        value.current_trace_path = Some(output_path);
        value.active_boundary = None;
        if execution.status.success() {
            value.phase = DemoPhase::Complete;
            value.next_step = BOUNDARIES.len();
        } else {
            value.phase = DemoPhase::Failed;
            value.command_error = nonempty_stderr(&execution.stderr);
        }
    })?;
    Ok(())
}

fn run_all(state: &Arc<Mutex<DemoState>>, config: &Config) -> Result<()> {
    reset_demo(state, config)?;
    for _ in 0..BOUNDARIES.len() {
        run_next(state, config)?;
        if matches!(snapshot_state(state)?.phase, DemoPhase::Faulted) {
            break;
        }
    }
    if matches!(snapshot_state(state)?.phase, DemoPhase::Faulted) {
        thread::sleep(Duration::from_millis(750));
        run_replay(state, config)?;
    }
    Ok(())
}

fn execute_crux(
    config: &Config,
    through: Option<&str>,
    replay: Option<&Path>,
    output_path: &Path,
) -> Result<std::process::Output> {
    let mut command = Command::new(&config.crux_bin);
    command
        .arg("run")
        .arg(&config.pipeline)
        .arg(&config.input)
        .arg("--quiet")
        .arg("--save-trace")
        .arg(output_path);
    if let Some(through) = through {
        command.arg("--through").arg(through);
    }
    if let Some(replay) = replay {
        command.arg("--replay").arg(replay);
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to execute crux CLI")?;
    let Some(status) = child
        .wait_timeout(Duration::from_secs(30))
        .context("failed while waiting for crux CLI")?
    else {
        child.kill().context("failed to terminate timed-out crux CLI")?;
        let _ = child.wait();
        bail!("crux CLI exceeded the 30 second demo deadline");
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut stream) = child.stdout.take() {
        stream.read_to_end(&mut stdout)?;
    }
    if let Some(mut stream) = child.stderr.take() {
        stream.read_to_end(&mut stderr)?;
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn read_trace(path: &Path) -> Result<StoredTrace> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read trace {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse trace {}", path.display()))
}

fn nonempty_stderr(stderr: &[u8]) -> Option<String> {
    let message = String::from_utf8_lossy(stderr).trim().to_string();
    (!message.is_empty()).then_some(message)
}

fn snapshot_state(state: &Arc<Mutex<DemoState>>) -> Result<DemoState> {
    state
        .lock()
        .map(|guard| guard.clone())
        .map_err(|_| anyhow::anyhow!("demo state lock poisoned"))
}

fn update_state(
    state: &Arc<Mutex<DemoState>>,
    state_file: &Path,
    update: impl FnOnce(&mut DemoState),
) -> Result<DemoState> {
    let mut guard = state
        .lock()
        .map_err(|_| anyhow::anyhow!("demo state lock poisoned"))?;
    update(&mut guard);
    refresh_controls(&mut guard);
    let current = guard.clone();
    write_state(state_file, &current)?;
    Ok(current)
}

fn refresh_controls(state: &mut DemoState) {
    let available = !state.busy && !state.finalize_in_flight;
    state.can_next = available
        && state.next_step < BOUNDARIES.len()
        && !matches!(
            state.phase,
            DemoPhase::Faulted | DemoPhase::Complete | DemoPhase::Failed
        );
    state.can_replay = available && matches!(state.phase, DemoPhase::Faulted);
    state.can_run_all = available;
    state.can_reset = available;
}

fn write_state(path: &Path, state: &DemoState) -> Result<()> {
    atomic_write(path, &serde_json::to_string_pretty(state)?)
}

fn respond_json(request: Request, status: StatusCode, value: &impl Serialize) -> Result<()> {
    let content_type = Header::from_bytes("content-type", "application/json")
        .map_err(|_| anyhow::anyhow!("invalid response content-type header"))?;
    let response = Response::from_string(serde_json::to_string(value)?)
        .with_status_code(status)
        .with_header(content_type);
    request
        .respond(response)
        .context("failed to write response")
}

fn respond_html(request: Request, html: &str) -> Result<()> {
    let content_type = Header::from_bytes("content-type", "text/html; charset=utf-8")
        .map_err(|_| anyhow::anyhow!("invalid response content-type header"))?;
    let response = Response::from_string(html)
        .with_status_code(StatusCode(200))
        .with_header(content_type);
    request
        .respond(response)
        .context("failed to write response")
}

fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, contents)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("failed to replace {}", path.display()))
}
