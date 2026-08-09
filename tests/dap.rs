//! End-to-end tests for `viml --dap` — the stdio Debug Adapter Protocol server.
//!
//! Each test drives the real binary over its stdin/stdout with
//! Content-Length-framed JSON, exactly as an IDE does, and asserts on the events
//! and responses that come back. These are the only tests that exercise the
//! debug compile (`compile_program_debug`), which is a separate code path from
//! the normal one and so can drift from it silently — as it had:
//! `:function` definitions were dropped from the debug program entirely, so no
//! user function existed under `--dap`.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// How long any single `wait_*` will block before declaring the adapter stuck.
const WAIT: Duration = Duration::from_secs(20);

fn viml_binary() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_viml") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("debug")
        .join("viml")
}

/// A live `viml --dap` process plus its framed-message channels.
///
/// Reading runs on its own thread feeding an `mpsc` channel: a blocking
/// `read_line` on the child's stdout pipe cannot be given a timeout, so an
/// adapter that simply goes quiet (the failure mode every one of these tests is
/// looking for) would hang the test binary instead of failing it.
struct Dap {
    child: Child,
    rx: Receiver<Value>,
    seq: i64,
    /// Messages received while waiting for something else, so a later `wait_*`
    /// can still find them.
    seen: Vec<Value>,
}

impl Dap {
    fn spawn() -> Self {
        let mut child = Command::new(viml_binary())
            .arg("--dap")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn viml --dap");
        let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Some(v) = read_message(&mut reader) {
                if tx.send(v).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            rx,
            seq: 1,
            seen: Vec::new(),
        }
    }

    fn request(&mut self, command: &str, arguments: Value) {
        let seq = self.seq;
        self.seq += 1;
        let msg =
            json!({ "seq": seq, "type": "request", "command": command, "arguments": arguments });
        let body = serde_json::to_vec(&msg).expect("encode request");
        let stdin = self.child.stdin.as_mut().expect("stdin");
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write header");
        stdin.write_all(&body).expect("write body");
        stdin.flush().expect("flush");
    }

    /// Take the next message from the reader thread, recording it in `seen`.
    /// `None` once the deadline passes or the adapter's stdout closes.
    fn next_message(&mut self, deadline: Instant) -> Option<Value> {
        let left = deadline.saturating_duration_since(Instant::now());
        match self.rx.recv_timeout(left) {
            Ok(v) => {
                self.seen.push(v.clone());
                Some(v)
            }
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    /// Read until `pred` matches a message, or the adapter exits / [`WAIT`] passes.
    fn wait_for(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        if let Some(v) = self.seen.iter().find(|v| pred(v)) {
            return v.clone();
        }
        let deadline = Instant::now() + WAIT;
        while let Some(v) = self.next_message(deadline) {
            if pred(&v) {
                return v;
            }
        }
        panic!("never saw {what}; messages so far: {:#?}", self.seen);
    }

    fn wait_event(&mut self, event: &str) -> Value {
        self.wait_for(&format!("event {event}"), |v| {
            v["type"] == "event" && v["event"] == event
        })
    }

    fn wait_response(&mut self, command: &str) -> Value {
        self.wait_for(&format!("response to {command}"), |v| {
            v["type"] == "response" && v["command"] == command
        })
    }

    /// Every `output` event body seen so far, concatenated.
    fn output(&self) -> String {
        self.seen
            .iter()
            .filter(|v| v["event"] == "output")
            .filter_map(|v| v["body"]["output"].as_str())
            .collect()
    }

    fn shutdown(&mut self) {
        self.request("disconnect", json!({}));
        // Drain whatever is left so the child sees EOF and exits.
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.next_message(deadline).is_some() {}
    }
}

/// Read one Content-Length-framed message from `r`. `None` at EOF.
fn read_message(r: &mut impl BufRead) -> Option<Value> {
    let mut len = 0usize;
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
            len = v.trim().parse().ok()?;
        }
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

impl Drop for Dap {
    /// Reap the adapter however the test ended. A failing assertion unwinds past
    /// [`Dap::shutdown`], and an adapter left holding a live stdin pipe never
    /// exits — the test binary then hangs instead of reporting the failure.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Write `src` to a temp `.vim` file and return its path (kept alive by `dir`).
fn script(dir: &tempfile::TempDir, src: &str) -> String {
    let p = dir.path().join("prog.vim");
    std::fs::write(&p, src).expect("write script");
    p.display().to_string()
}

const FN_SCRIPT: &str =
    "function! Add(a, b)\n  return a:a + a:b\nendfunction\necho Add(2, 3)\necho \"done\"\n";

/// A `:function` in a script debugged under `--dap` must be DEFINED, so calling
/// it produces the same output as running the script normally (and as vim).
///
/// Regression: the debug compile skipped every `Stmt::Function` and discarded
/// the program's `funcs`/`deferred_funcs`, so `echo Add(2, 3)` printed nothing
/// and only the trailing `echo "done"` reached the client.
#[test]
fn dap_defines_user_functions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = script(&dir, FN_SCRIPT);
    let mut dap = Dap::spawn();
    dap.request("initialize", json!({}));
    dap.wait_event("initialized");
    dap.request("configurationDone", json!({}));
    dap.request("launch", json!({ "program": path }));
    dap.wait_event("terminated");
    assert_eq!(dap.output(), "5\ndone\n");
    dap.shutdown();
}

/// A line breakpoint on a statement INSIDE a `:function` body must stop there.
///
/// Regression: markers were emitted only by the debug compile's own top-level
/// loop, so function bodies carried none and were unbreakable.
#[test]
fn dap_breakpoint_inside_function_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = script(&dir, FN_SCRIPT);
    let mut dap = Dap::spawn();
    dap.request("initialize", json!({}));
    dap.wait_event("initialized");
    dap.request(
        "setBreakpoints",
        json!({ "source": { "path": path }, "breakpoints": [{ "line": 2 }] }),
    );
    dap.wait_response("setBreakpoints");
    dap.request("configurationDone", json!({}));
    dap.request("launch", json!({ "program": path }));
    let stopped = dap.wait_event("stopped");
    assert_eq!(stopped["body"]["reason"], "breakpoint");
    dap.request("stackTrace", json!({ "threadId": 1 }));
    let st = dap.wait_response("stackTrace");
    assert_eq!(st["body"]["stackFrames"][0]["line"], 2);
    dap.request("continue", json!({ "threadId": 1 }));
    dap.wait_event("terminated");
    assert_eq!(dap.output(), "5\ndone\n");
    dap.shutdown();
}
