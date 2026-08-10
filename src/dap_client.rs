//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! EXTENSION — NO `vendor/` COUNTERPART. A DAP *client*: drives a `viml --dap`
//! child over its stdin/stdout with Content-Length-framed JSON, exactly as an
//! IDE does. Development/testing surface only — nothing in the language runtime
//! calls it.
//!
//! It lives here, in the library, because it has two consumers — `tests/dap.rs`
//! and the `--dap` mode of `src/bin/fuzz_parity.rs` — and a debug session driven
//! two slightly different ways is a session neither one really covers.
//!
//! ## A hanging client is a failing test that never fails
//!
//! A blocking `read_line` on the child's stdout cannot be given a timeout, so an
//! adapter that simply goes quiet — the failure mode every one of these checks
//! is looking for — would hang the caller instead of failing it. Reading
//! therefore runs on its own thread feeding an `mpsc` channel, and every wait is
//! a `recv_timeout` against a deadline. [`Dap`]'s `Drop` kills the child however
//! the caller left, since an adapter still holding a live stdin pipe never
//! exits on its own.
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// How long any single wait blocks before declaring the adapter stuck.
pub const WAIT: Duration = Duration::from_secs(20);

/// Path to the `viml` binary to debug: the one Cargo built for this test binary
/// when there is one, else `target/debug/viml`.
pub fn viml_binary() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_viml") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("debug")
        .join("viml")
}

/// A live `viml --dap` process plus its framed-message channels.
pub struct Dap {
    child: Child,
    rx: Receiver<Value>,
    seq: i64,
    /// Messages received while waiting for something else, so a later wait can
    /// still find them.
    pub seen: Vec<Value>,
}

impl Dap {
    /// Spawn [`viml_binary`] under `--dap`.
    pub fn spawn() -> Self {
        Self::spawn_binary(&viml_binary())
    }

    /// Spawn a specific binary under `--dap`.
    pub fn spawn_binary(bin: &Path) -> Self {
        let mut child = Command::new(bin)
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

    /// Send one request; returns its `seq`, for [`Self::try_wait_response_seq`].
    /// Panics if the adapter's stdin has closed.
    pub fn request(&mut self, command: &str, arguments: Value) -> i64 {
        let seq = self.seq;
        self.seq += 1;
        let msg =
            json!({ "seq": seq, "type": "request", "command": command, "arguments": arguments });
        let body = serde_json::to_vec(&msg).expect("encode request");
        let stdin = self.child.stdin.as_mut().expect("stdin");
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write header");
        stdin.write_all(&body).expect("write body");
        stdin.flush().expect("flush");
        seq
    }

    /// Send one request, reporting a closed pipe instead of panicking — the
    /// fuzzer drives adapters that may have died on the case under test.
    pub fn try_request(&mut self, command: &str, arguments: Value) -> Result<i64, String> {
        let seq = self.seq;
        self.seq += 1;
        let msg =
            json!({ "seq": seq, "type": "request", "command": command, "arguments": arguments });
        let body = serde_json::to_vec(&msg).map_err(|e| e.to_string())?;
        let stdin = self.child.stdin.as_mut().ok_or("adapter stdin closed")?;
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).map_err(|e| e.to_string())?;
        stdin.write_all(&body).map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
        Ok(seq)
    }

    /// Wait for the response to ONE specific request, matched on `request_seq`.
    ///
    /// [`Self::wait_response`] matches on the command name and searches
    /// [`Self::seen`] first, so asking twice for the same command hands back the
    /// FIRST answer — fine for a scripted test that asks once, wrong for a loop
    /// that asks at every stop. Correlating on the seq is what makes a repeated
    /// request answerable.
    pub fn try_wait_response_seq(&mut self, seq: i64, timeout: Duration) -> Option<Value> {
        self.try_wait_for(timeout, |v| {
            v["type"] == "response" && v["request_seq"] == seq
        })
    }

    /// Take the next message from the reader thread, recording it in
    /// [`Self::seen`]. `None` once the deadline passes or stdout closes.
    pub fn next_message(&mut self, deadline: Instant) -> Option<Value> {
        let left = deadline.saturating_duration_since(Instant::now());
        match self.rx.recv_timeout(left) {
            Ok(v) => {
                self.seen.push(v.clone());
                Some(v)
            }
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    /// Read until `pred` matches, or the adapter exits / `timeout` passes.
    /// `None` rather than a panic, so a caller that has to keep going can.
    pub fn try_wait_for(
        &mut self,
        timeout: Duration,
        pred: impl Fn(&Value) -> bool,
    ) -> Option<Value> {
        if let Some(v) = self.seen.iter().find(|v| pred(v)) {
            return Some(v.clone());
        }
        let deadline = Instant::now() + timeout;
        while let Some(v) = self.next_message(deadline) {
            if pred(&v) {
                return Some(v);
            }
        }
        None
    }

    /// Read forward from `cursor` until `pred` matches, advancing `cursor` past
    /// the match. Unlike [`Self::try_wait_for`], asking twice returns the second
    /// occurrence — which is what a loop that waits for a `stopped` event at
    /// every stop needs. `None` on timeout or EOF.
    pub fn wait_next(
        &mut self,
        cursor: &mut usize,
        timeout: Duration,
        pred: impl Fn(&Value) -> bool,
    ) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            while *cursor < self.seen.len() {
                let v = self.seen[*cursor].clone();
                *cursor += 1;
                if pred(&v) {
                    return Some(v);
                }
            }
            self.next_message(deadline)?;
        }
    }

    /// Read until `pred` matches a message, or the adapter exits / [`WAIT`]
    /// passes — in which case it panics with everything seen so far.
    pub fn wait_for(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        match self.try_wait_for(WAIT, pred) {
            Some(v) => v,
            None => panic!("never saw {what}; messages so far: {:#?}", self.seen),
        }
    }

    /// Wait for a named event.
    pub fn wait_event(&mut self, event: &str) -> Value {
        self.wait_for(&format!("event {event}"), |v| {
            v["type"] == "event" && v["event"] == event
        })
    }

    /// Wait for the response to a named request.
    pub fn wait_response(&mut self, command: &str) -> Value {
        self.wait_for(&format!("response to {command}"), |v| {
            v["type"] == "response" && v["command"] == command
        })
    }

    /// Every `output` event body seen so far, concatenated.
    pub fn output(&self) -> String {
        self.seen
            .iter()
            .filter(|v| v["event"] == "output")
            .filter_map(|v| v["body"]["output"].as_str())
            .collect()
    }

    /// How many `stopped` events have arrived so far.
    pub fn stops(&self) -> usize {
        self.seen.iter().filter(|v| v["event"] == "stopped").count()
    }

    /// Disconnect and drain, so the child sees EOF and exits.
    pub fn shutdown(&mut self) {
        let _ = self.try_request("disconnect", json!({}));
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.next_message(deadline).is_some() {}
    }
}

impl Drop for Dap {
    /// Reap the adapter however the caller ended. A failing assertion unwinds
    /// past [`Dap::shutdown`], and an adapter left holding a live stdin pipe
    /// never exits — the caller then hangs instead of reporting the failure.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Read one Content-Length-framed message from `r`. `None` at EOF.
pub fn read_message(r: &mut impl BufRead) -> Option<Value> {
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
