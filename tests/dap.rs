//! End-to-end tests for `viml --dap` — the stdio Debug Adapter Protocol server.
//!
//! Each test drives the real binary over its stdin/stdout with
//! Content-Length-framed JSON, exactly as an IDE does, and asserts on the events
//! and responses that come back. These are the only tests that exercise the
//! debug compile (`compile_program_debug`), which is a separate code path from
//! the normal one and so can drift from it silently — as it had:
//! `:function` definitions were dropped from the debug program entirely, so no
//! user function existed under `--dap`.
//!
//! The client itself is [`vimlrs::dap_client`], shared with the `--dap` mode of
//! `fuzz-parity`; its module docs explain why every wait has a deadline and why
//! the child is killed on drop (a stuck adapter must fail these tests, not hang
//! them).

use serde_json::json;
use vimlrs::dap_client::Dap;

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

/// `setFunctionBreakpoints` must be advertised, accepted, and honoured: naming a
/// function stops at the first statement of its body, with the frame named after
/// the function.
#[test]
fn dap_function_breakpoint_stops_at_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = script(&dir, FN_SCRIPT);
    let mut dap = Dap::spawn();
    dap.request("initialize", json!({}));
    let caps = dap.wait_response("initialize");
    assert_eq!(caps["body"]["supportsFunctionBreakpoints"], true);
    dap.request(
        "setFunctionBreakpoints",
        json!({ "breakpoints": [{ "name": "Add" }] }),
    );
    let set = dap.wait_response("setFunctionBreakpoints");
    assert_eq!(set["body"]["breakpoints"][0]["verified"], true);
    dap.request("configurationDone", json!({}));
    dap.request("launch", json!({ "program": path }));
    let stopped = dap.wait_event("stopped");
    assert_eq!(stopped["body"]["reason"], "function breakpoint");
    dap.request("stackTrace", json!({ "threadId": 1 }));
    let st = dap.wait_response("stackTrace");
    // Line 2 is `return a:a + a:b` — the body's first statement, not the call.
    assert_eq!(st["body"]["stackFrames"][0]["line"], 2);
    assert_eq!(st["body"]["stackFrames"][0]["name"], "Add");
    dap.request("continue", json!({ "threadId": 1 }));
    dap.wait_event("terminated");
    assert_eq!(dap.output(), "5\ndone\n");
    dap.shutdown();
}

/// Reference output for [`FUNCREF_SCRIPT`], from
/// `VIM - Vi IMproved 9.2 (2026 Feb 14, compiled Aug 02 2026 19:00:41)`:
///
/// ```text
/// $ vim -es -u NONE -i NONE -c 'redir! > out' -c 'source fr.vim' \
///     -c 'redir END' -c 'qa!'
/// $ xxd out
/// 00000000: 0a68 6920 610a 6869 2062 0a65 6e64       .hi a.hi b.end
/// ```
///
/// `-i NONE` is load-bearing, not decoration. Without it — and this command was
/// written without it — vim reads `~/.viminfo` whenever it is nocompatible, so
/// the editor starts with the developer's own registers, search pattern and
/// command history. On this machine `vim -N -u NONE -es` reports
/// `strlen(getreg('"'))` 18, `histnr('cmd')` 100 and `len(v:oldfiles)` 100 where
/// `-i NONE` gives 0, -1 and 0. The script below reads none of those, so its
/// output was unaffected, but any reference recorded that way is recording the
/// machine it ran on.
const FUNCREF_SCRIPT: &str = "function! Greet(who)\n  echo \"hi \" . a:who\nendfunction\nlet F = function('Greet')\ncall F('a')\ncall Greet('b')\necho \"end\"\n";

/// A function breakpoint fires however the function is reached — through a
/// Funcref as well as by name.
///
/// This is the property that makes one hook site sufficient: every caller
/// (`Foo()`, `:call`, a Funcref, an AOP intercept's re-run of the original)
/// enters the body through `call_user_function_raw`, so the breakpoint is
/// checked exactly once, in one place. A per-call-site check would have missed
/// the Funcref path.
#[test]
fn dap_function_breakpoint_fires_through_funcref() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = script(&dir, FUNCREF_SCRIPT);
    let mut dap = Dap::spawn();
    dap.request("initialize", json!({}));
    dap.wait_event("initialized");
    dap.request(
        "setFunctionBreakpoints",
        json!({ "breakpoints": [{ "name": "Greet" }] }),
    );
    dap.wait_response("setFunctionBreakpoints");
    dap.request("configurationDone", json!({}));
    dap.request("launch", json!({ "program": path }));

    // `call F('a')` — reached through the Funcref.
    let first = dap.wait_event("stopped");
    assert_eq!(first["body"]["reason"], "function breakpoint");
    dap.request("continue", json!({ "threadId": 1 }));

    // `call Greet('b')` — reached by name. Two `stopped` events in all.
    let deadline = std::time::Instant::now() + vimlrs::dap_client::WAIT;
    while dap.stops() < 2 {
        assert!(
            dap.next_message(deadline).is_some(),
            "only one function-breakpoint stop; the Funcref and by-name calls \
             should each produce one: {:#?}",
            dap.seen
        );
    }
    dap.request("continue", json!({ "threadId": 1 }));
    dap.wait_event("terminated");
    assert_eq!(dap.output(), "hi a\nhi b\nend\n");
    dap.shutdown();
}

/// Two calls deep: `Bar` (line 2) ← `Foo` (line 6) ← script (line 9).
///
/// The layout matters — every frame sits on a DIFFERENT line, so a stack that
/// reported the same line three times, or the innermost line three times, fails
/// on the value and not merely on the count.
const NESTED_SCRIPT: &str = "\
function! Bar()
  echo \"in bar\"
  return 2
endfunction
function! Foo()
  return Bar() + 1
endfunction
echo \"start\"
echo Foo()
echo \"end\"
";

/// `stackTrace` must report the WHOLE backtrace, innermost first, each frame on
/// its own line — not one synthetic frame however deep execution is.
///
/// Reference, from [`NESTED_SCRIPT`] written to `bt.vim` and stopped two calls
/// deep in real vim. Its indices count down to `->0` at the innermost, and its
/// in-function line numbers are body-relative where DAP frames are file-absolute
/// — `Foo[1]` is `return Bar() + 1`, file line 6:
///
/// ```text
/// $ printf 'backtrace\ncont\n' | vim -es -u NONE -i NONE \
///     -c 'breakadd func Bar' -c 'verbose source bt.vim' -c 'qa!'
/// start
/// Breakpoint in "Bar" line 1
/// Entering Debug mode.  Type "cont" to continue.
/// command line..script bt.vim[9]..function Foo[1]..Bar
/// line 1: echo "in bar"
/// >backtrace
///   3 command line
///   2 script bt.vim[9]
///   1 function Foo[1]
/// ->0 Bar
/// ```
#[test]
fn dap_stack_trace_reports_every_frame() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = script(&dir, NESTED_SCRIPT);
    let mut dap = Dap::spawn();
    dap.request("initialize", json!({}));
    dap.wait_event("initialized");
    // Line 2 is `echo "in bar"`, the first statement of the innermost body.
    dap.request(
        "setBreakpoints",
        json!({ "source": { "path": path }, "breakpoints": [{ "line": 2 }] }),
    );
    dap.wait_response("setBreakpoints");
    dap.request("configurationDone", json!({}));
    dap.request("launch", json!({ "program": path }));
    dap.wait_event("stopped");

    dap.request("stackTrace", json!({ "threadId": 1 }));
    let st = dap.wait_response("stackTrace");
    let frames = st["body"]["stackFrames"]
        .as_array()
        .expect("stackFrames array")
        .clone();
    let got: Vec<(String, i64)> = frames
        .iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap_or_default().to_string(),
                f["line"].as_i64().unwrap_or(-1),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("Bar".to_string(), 2),    // stopped here
            ("Foo".to_string(), 6),    // `return Bar() + 1`
            ("script".to_string(), 9), // `echo Foo()`
        ],
        "backtrace should be innermost-first with each frame on its own line"
    );
    assert_eq!(st["body"]["totalFrames"], 3);
    // Ids are 1-based positions, so a client can ask for `scopes` of a frame.
    assert_eq!(frames[0]["id"], 1);
    assert_eq!(frames[2]["id"], 3);

    // The client's paging window must be honoured, with `totalFrames` still the
    // whole stack — that is how the client knows more frames exist.
    dap.request(
        "stackTrace",
        json!({ "threadId": 1, "startFrame": 1, "levels": 1 }),
    );
    let page = dap.wait_for("paged stackTrace", |v| {
        v["command"] == "stackTrace"
            && v["body"]["stackFrames"]
                .as_array()
                .is_some_and(|a| a.len() == 1)
    });
    assert_eq!(page["body"]["stackFrames"][0]["name"], "Foo");
    assert_eq!(page["body"]["stackFrames"][0]["id"], 2);
    assert_eq!(page["body"]["totalFrames"], 3);

    dap.request("continue", json!({ "threadId": 1 }));
    dap.wait_event("terminated");
    assert_eq!(dap.output(), "start\nin bar\n3\nend\n");
    dap.shutdown();
}

/// Drive to the first stop on `line`, then send `verb` and report where the next
/// stop landed as `(frame name, line, depth)`.
fn step_from(src: &str, bp_line: u32, verb: &str) -> (String, i64, usize) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = script(&dir, src);
    let mut dap = Dap::spawn();
    dap.request("initialize", json!({}));
    dap.wait_event("initialized");
    dap.request(
        "setBreakpoints",
        json!({ "source": { "path": path }, "breakpoints": [{ "line": bp_line }] }),
    );
    dap.wait_response("setBreakpoints");
    dap.request("configurationDone", json!({}));
    dap.request("launch", json!({ "program": path }));
    dap.wait_event("stopped");

    dap.request(verb, json!({ "threadId": 1 }));
    // The stop we want is the SECOND one; `terminated` instead means the verb
    // ran off the end of the program, which is a wrong answer, not a hang.
    let deadline = std::time::Instant::now() + vimlrs::dap_client::WAIT;
    while dap.stops() < 2 {
        let msg = dap.next_message(deadline);
        assert!(
            msg.is_some(),
            "`{verb}` from line {bp_line} produced no second stop: {:#?}",
            dap.seen
        );
        if msg.is_some_and(|m| m["event"] == "terminated") {
            panic!("`{verb}` from line {bp_line} ran to termination instead of stopping");
        }
    }
    dap.request("stackTrace", json!({ "threadId": 1 }));
    let st = dap.wait_response("stackTrace");
    let frames = st["body"]["stackFrames"]
        .as_array()
        .expect("frames")
        .clone();
    let out = (
        frames[0]["name"].as_str().unwrap_or_default().to_string(),
        frames[0]["line"].as_i64().unwrap_or(-1),
        frames.len() - 1, // depth: script level is 0
    );
    dap.shutdown();
    out
}

/// `stepIn` on a statement that CALLS must land inside the callee.
///
/// Reference, from [`NESTED_SCRIPT`] as `bt.vim`, stopped on `Foo`'s
/// `return Bar() + 1` (file line 6, body line 1) — `step` descends into `Bar`:
///
/// ```text
/// $ printf 'step\ncont\n' | vim -es -u NONE -i NONE \
///     -c 'breakadd func 1 Foo' -c 'verbose source bt.vim' -c 'qa!'
/// command line..script bt.vim[9]..function Foo
/// line 1: return Bar() + 1
/// >step
/// command line..script bt.vim[9]..function Foo[1]..Bar
/// line 1: echo "in bar"
/// ```
#[test]
fn dap_step_in_enters_the_callee() {
    // Stop on line 6 (`return Bar() + 1`) inside `Foo`, then step in.
    let (name, line, depth) = step_from(NESTED_SCRIPT, 6, "stepIn");
    assert_eq!(
        (name.as_str(), line, depth),
        ("Bar", 2, 2),
        "stepIn should reach `Bar`'s first statement, one frame deeper"
    );
}

/// `next` on a statement that CALLS must step OVER it, staying at the same depth.
///
/// Reference, from [`NESTED_SCRIPT`] as `bt.vim`, stopped at script level on
/// `echo Foo()` (line 9) — `next` runs the whole call and lands on line 10,
/// still at script level. `in bar` / `3` are what the stepped-over call printed:
///
/// ```text
/// $ printf 'next\ncont\n' | vim -es -u NONE -i NONE \
///     -c 'breakadd file 9 bt.vim' -c 'verbose source bt.vim' -c 'qa!'
/// command line..script bt.vim
/// line 9: echo Foo()
/// >next
/// in bar
/// 3
/// command line..script bt.vim
/// line 10: echo "end"
/// ```
///
/// This is the target that was aliased to `stepIn`: `next` used to land on
/// `Bar`'s body exactly like `stepIn` did.
#[test]
fn dap_next_steps_over_the_call() {
    // Stop on line 9 (`echo Foo()`) at script level, then step over the call.
    let (name, line, depth) = step_from(NESTED_SCRIPT, 9, "next");
    assert_eq!(
        (name.as_str(), line, depth),
        ("script", 10, 0),
        "next should reach the following script line without entering `Foo`"
    );
}

/// `stepOut` must return to the CALLER, skipping the rest of the callee.
///
/// vim's `finish` makes one extra stop on the callee's synthetic `line 2: End of
/// function` first; there is no end-of-body statement to mark here and DAP
/// specifies the caller, so `viml` goes straight there — the deviation is stated
/// on the handler in `src/dap.rs`. That extra stop, from [`NESTED_SCRIPT`] as
/// `bt.vim`, is the only difference:
///
/// ```text
/// $ printf 'finish\ncont\n' | vim -es -u NONE -i NONE \
///     -c 'breakadd func 1 Bar' -c 'verbose source bt.vim' -c 'qa!'
/// command line..script bt.vim[9]..function Foo[1]..Bar
/// line 1: echo "in bar"
/// >finish
/// in bar
/// command line..script bt.vim[9]..function Foo[1]..Bar
/// line 2: End of function
/// ```
///
/// This is the target that was aliased to `next`: `stepOut` from two frames deep
/// used to land on the *next line of the same function*.
#[test]
fn dap_step_out_returns_to_the_caller() {
    // Stop on line 2, inside `Bar`, two frames deep. Line 3 (`return 2`) is the
    // rest of `Bar` — stepping out must not stop there.
    let (name, line, depth) = step_from(NESTED_SCRIPT, 2, "stepOut");
    assert_eq!(
        (name.as_str(), line, depth),
        ("script", 10, 0),
        "stepOut of `Bar` should skip the rest of it AND the rest of `Foo`, \
         which has nothing left to run, landing back at script level"
    );
}

/// A `|`-joined line holds two commands, and vim's debugger is command-oriented:
/// it stops on each. Measured on `VIM - Vi IMproved 9.2 …`:
///
/// ```text
/// >step
/// line 1: let a = 1 | let b = 2
/// >step
/// line 1: let b = 2
/// ```
///
/// awkrs's `should_stop` suppresses the second stop with a same-line guard
/// (`awkrs/src/debugger.rs:159`) because its debugger is line-oriented. Porting
/// that guard here would have made `viml` skip a command vim stops on, so it is
/// deliberately absent — this test is what keeps it absent.
#[test]
fn dap_steps_once_per_command_not_once_per_line() {
    const BAR_SCRIPT: &str = "let a = 1 | let b = 2\necho a + b\n";
    let dir = tempfile::tempdir().expect("tempdir");
    let path = script(&dir, BAR_SCRIPT);
    let mut dap = Dap::spawn();
    dap.request("initialize", json!({}));
    dap.wait_event("initialized");
    dap.request(
        "setBreakpoints",
        json!({ "source": { "path": path }, "breakpoints": [{ "line": 1 }] }),
    );
    dap.wait_response("setBreakpoints");
    dap.request("configurationDone", json!({}));
    dap.request("launch", json!({ "program": path }));
    dap.wait_event("stopped");

    // Second command of line 1 — still line 1.
    dap.request("next", json!({ "threadId": 1 }));
    let deadline = std::time::Instant::now() + vimlrs::dap_client::WAIT;
    while dap.stops() < 2 {
        assert!(
            dap.next_message(deadline).is_some(),
            "`next` on a `|`-joined line must stop on its second command: {:#?}",
            dap.seen
        );
    }
    dap.request("stackTrace", json!({ "threadId": 1 }));
    let st = dap.wait_response("stackTrace");
    assert_eq!(
        st["body"]["stackFrames"][0]["line"], 1,
        "the second stop is the second command of line 1"
    );
    dap.request("continue", json!({ "threadId": 1 }));
    dap.wait_event("terminated");
    assert_eq!(dap.output(), "3\n");
    dap.shutdown();
}
