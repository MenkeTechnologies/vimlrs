//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `csrc/` COUNTERPART. Debug Adapter Protocol server (stdio) —
//! `vimlrs --dap`. Mirrors zshrs's `dap.rs` architecture: the script runs
//! IN-PROCESS on an executor thread; the compiler emits a `SET_LINENO` marker
//! before each statement whose handler calls [`check_line`], which consults the
//! shared breakpoint state and condvar-waits to pause. While paused, the
//! executor thread also services `variables` / `evaluate` requests (so they read
//! the executor's own `globvardict`), then resumes when the client sends
//! `continue` / `next`.
//!
//! DAP messages are Content-Length-framed JSON. Program `:echo` output is
//! captured and streamed as `output` events at each pause / at termination, so
//! it never corrupts the JSON-RPC channel on stdout.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use std::collections::{HashSet, VecDeque};
use std::io::{self, BufRead, BufReader, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

use serde_json::{json, Value};

/// What the client asked a paused executor to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resume {
    /// Run until the next breakpoint.
    Continue,
    /// Stop again at the next statement.
    Next,
}

/// A request that must be answered on the executor thread while paused
/// (so it reads the executor's `globvardict`).
#[derive(Debug)]
enum Pending {
    /// `variables` request: response seq.
    Variables(i64),
    /// `evaluate` request: response seq + expression.
    Evaluate(i64, String),
}

#[derive(Default)]
struct DapState {
    breakpoints: HashSet<u32>,
    resume: Option<Resume>,
    stepping: bool,
    pause_requested: bool,
    terminated: bool,
    current_line: u32,
    pending: VecDeque<Pending>,
}

struct DapShared {
    state: Mutex<DapState>,
    cv: Condvar,
}

static SHARED: OnceLock<DapShared> = OnceLock::new();
static SEQ: AtomicI64 = AtomicI64::new(1);

fn shared() -> &'static DapShared {
    SHARED.get_or_init(|| DapShared {
        state: Mutex::new(DapState::default()),
        cv: Condvar::new(),
    })
}

// ── DAP transport ──

fn next_seq() -> i64 {
    SEQ.fetch_add(1, Ordering::SeqCst)
}

/// Send a framed JSON message on stdout (locks stdout — both threads send).
fn send(msg: Value) {
    let body = serde_json::to_string(&msg).unwrap_or_default();
    let out = io::stdout();
    let mut lock = out.lock();
    let _ = write!(lock, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = lock.flush();
}

fn send_response(request_seq: i64, command: &str, body: Value) {
    send(json!({
        "seq": next_seq(), "type": "response", "request_seq": request_seq,
        "success": true, "command": command, "body": body,
    }));
}

fn send_event(event: &str, body: Value) {
    send(json!({ "seq": next_seq(), "type": "event", "event": event, "body": body }));
}

/// Stream any buffered program output as a DAP `output` event.
fn flush_output() {
    let out = crate::fusevm_bridge::capture_drain();
    if !out.is_empty() {
        send_event("output", json!({ "category": "stdout", "output": out }));
    }
}

/// Read one Content-Length-framed message from `r`. `None` on EOF.
fn read_message(r: &mut impl BufRead) -> Option<Value> {
    let mut content_len = 0usize;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(v) = trimmed.strip_prefix("Content-Length:") {
            content_len = v.trim().parse().ok()?;
        }
    }
    let mut buf = vec![0u8; content_len];
    r.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

// ── executor-side pause hook ──

/// Called (via the `SET_LINENO` builtin) before each statement in a debug build.
/// Pauses the executor at breakpoints / when stepping, servicing variable and
/// evaluate requests while stopped.
pub fn check_line(line: u32) {
    let Some(sh) = SHARED.get() else {
        return; // not running under --dap
    };
    flush_output();
    let mut st = sh.state.lock().unwrap();
    st.current_line = line;
    let stop = st.breakpoints.contains(&line) || st.stepping || st.pause_requested;
    if !stop {
        return;
    }
    st.pause_requested = false;
    st.stepping = false;
    let reason = if st.breakpoints.contains(&line) {
        "breakpoint"
    } else {
        "step"
    };
    drop(st);
    send_event(
        "stopped",
        json!({ "reason": reason, "threadId": 1, "allThreadsStopped": true }),
    );

    // Wait for resume, servicing var/eval requests on this (executor) thread.
    let mut st = sh.state.lock().unwrap();
    loop {
        while let Some(req) = st.pending.pop_front() {
            drop(st);
            match req {
                Pending::Variables(seq) => answer_variables(seq),
                Pending::Evaluate(seq, expr) => answer_evaluate(seq, &expr),
            }
            st = sh.state.lock().unwrap();
        }
        if st.terminated {
            return;
        }
        if let Some(r) = st.resume.take() {
            if r == Resume::Next {
                st.stepping = true;
            }
            return;
        }
        st = sh.cv.wait(st).unwrap();
    }
}

fn answer_variables(request_seq: i64) {
    let vars: Vec<Value> = crate::fusevm_bridge::dap_globals()
        .into_iter()
        .map(|(name, value)| json!({ "name": name, "value": value, "variablesReference": 0 }))
        .collect();
    send_response(request_seq, "variables", json!({ "variables": vars }));
}

fn answer_evaluate(request_seq: i64, expr: &str) {
    let result = crate::fusevm_bridge::dap_eval_var(expr.trim())
        .unwrap_or_else(|| "<not a variable in scope>".to_string());
    send_response(request_seq, "evaluate", json!({ "result": result, "variablesReference": 0 }));
}

// ── server entry point ──

/// Run the DAP server on stdio until `disconnect`. The script (from the
/// `launch` request) runs on a spawned executor thread; this thread keeps
/// reading client requests so breakpoints can be honored.
pub fn run_stdio() -> Result<(), String> {
    shared(); // initialize the global so check_line activates
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut executor: Option<std::thread::JoinHandle<()>> = None;

    while let Some(msg) = read_message(&mut reader) {
        let command = msg.get("command").and_then(Value::as_str).unwrap_or("").to_string();
        let seq = msg.get("seq").and_then(Value::as_i64).unwrap_or(0);
        let args = msg.get("arguments").cloned().unwrap_or(Value::Null);
        match command.as_str() {
            "initialize" => {
                send_response(
                    seq,
                    "initialize",
                    json!({
                        "supportsConfigurationDoneRequest": true,
                        "supportsEvaluateForHovers": true,
                    }),
                );
                send_event("initialized", json!({}));
            }
            "setBreakpoints" => {
                let lines: Vec<u32> = args
                    .get("breakpoints")
                    .and_then(Value::as_array)
                    .map(|bps| {
                        bps.iter()
                            .filter_map(|b| b.get("line").and_then(Value::as_u64).map(|l| l as u32))
                            .collect()
                    })
                    .unwrap_or_default();
                {
                    let mut st = shared().state.lock().unwrap();
                    st.breakpoints = lines.iter().copied().collect();
                }
                let verified: Vec<Value> = lines
                    .iter()
                    .map(|l| json!({ "verified": true, "line": l }))
                    .collect();
                send_response(seq, "setBreakpoints", json!({ "breakpoints": verified }));
            }
            "setExceptionBreakpoints" => {
                send_response(seq, "setExceptionBreakpoints", json!({}));
            }
            "configurationDone" => send_response(seq, "configurationDone", json!({})),
            "launch" => {
                send_response(seq, "launch", json!({}));
                let program = args
                    .get("program")
                    .or_else(|| args.get("script"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let source = match program.as_deref() {
                    Some(p) => std::fs::read_to_string(p).unwrap_or_default(),
                    None => args.get("source").and_then(Value::as_str).unwrap_or("").to_string(),
                };
                executor = Some(std::thread::spawn(move || {
                    // Capture `:echo` on THIS (executor) thread so it streams as
                    // DAP `output` events instead of corrupting the stdout channel.
                    crate::fusevm_bridge::capture_begin();
                    let _ = crate::fusevm_bridge::eval_source_debug(&source);
                    flush_output();
                    send_event("terminated", json!({}));
                    send_event("exited", json!({ "exitCode": 0 }));
                }));
            }
            "threads" => send_response(
                seq,
                "threads",
                json!({ "threads": [{ "id": 1, "name": "main" }] }),
            ),
            "stackTrace" => {
                let line = shared().state.lock().unwrap().current_line;
                send_response(
                    seq,
                    "stackTrace",
                    json!({
                        "stackFrames": [{
                            "id": 1, "name": "script", "line": line, "column": 1,
                        }],
                        "totalFrames": 1,
                    }),
                );
            }
            "scopes" => send_response(
                seq,
                "scopes",
                json!({ "scopes": [{
                    "name": "Globals", "variablesReference": 1000, "expensive": false,
                }] }),
            ),
            "variables" => {
                // Serviced on the executor thread while paused (reads its globals).
                let mut st = shared().state.lock().unwrap();
                st.pending.push_back(Pending::Variables(seq));
                shared().cv.notify_all();
            }
            "evaluate" => {
                let expr = args.get("expression").and_then(Value::as_str).unwrap_or("").to_string();
                let mut st = shared().state.lock().unwrap();
                st.pending.push_back(Pending::Evaluate(seq, expr));
                shared().cv.notify_all();
            }
            "continue" => {
                {
                    let mut st = shared().state.lock().unwrap();
                    st.resume = Some(Resume::Continue);
                }
                shared().cv.notify_all();
                send_response(seq, "continue", json!({ "allThreadsContinued": true }));
            }
            "next" | "stepIn" | "stepOut" => {
                {
                    let mut st = shared().state.lock().unwrap();
                    st.resume = Some(Resume::Next);
                }
                shared().cv.notify_all();
                send_response(seq, &command, json!({}));
            }
            "pause" => {
                shared().state.lock().unwrap().pause_requested = true;
                send_response(seq, "pause", json!({}));
            }
            "disconnect" | "terminate" => {
                {
                    let mut st = shared().state.lock().unwrap();
                    st.terminated = true;
                    st.resume = Some(Resume::Continue);
                }
                shared().cv.notify_all();
                send_response(seq, &command, json!({}));
                break;
            }
            _ => send_response(seq, &command, json!({})),
        }
    }

    if let Some(h) = executor {
        let _ = h.join();
    }
    Ok(())
}
