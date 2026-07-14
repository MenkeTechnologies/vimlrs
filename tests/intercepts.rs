//! Integration tests for the AOP command-intercept extension
//! (vimlrs/zshrs-original; no Vim counterpart). Each test drives a self-checking
//! VimL script through the built `viml` binary: the script exercises
//! before/after/around advice on a user-function call, asserts the observed
//! behavior with the built-in `assert_*` framework, and its epilogue `throw`s
//! (→ non-zero exit) if `v:errors` is non-empty. On success it prints `ALL_OK`.
//!
//! These run headless with no editor installed — only the `CARGO_BIN_EXE_viml`
//! binary Cargo builds for the test — so they pass in CI on Linux.

use std::io::Write;
use std::process::Command;

/// Write `src` to a temp `.vim`, run it through the built `viml`, and return
/// `(success, stdout, stderr)`.
fn run_viml(src: &str) -> (bool, String, String) {
    let mut f = tempfile::Builder::new()
        .suffix(".vim")
        .tempfile()
        .expect("temp .vim");
    f.write_all(src.as_bytes()).expect("write script");
    let path = f.path().to_path_buf();
    let out = Command::new(env!("CARGO_BIN_EXE_viml"))
        .arg(&path)
        .output()
        .expect("run viml");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Assert the script ran clean (exit 0, printed `ALL_OK`, no `E<num>:` error).
fn assert_ok(src: &str) {
    let (ok, stdout, stderr) = run_viml(src);
    assert!(
        ok && stdout.contains("ALL_OK"),
        "script did not pass.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // No Vim error line leaked to stderr.
    for line in stderr.lines() {
        assert!(
            !is_vim_error(line),
            "unexpected Vim error on stderr: {line}"
        );
    }
}

/// A line is a Vim error if it contains `E<digits>:`.
fn is_vim_error(line: &str) -> bool {
    line.match_indices('E').any(|(i, _)| {
        let rest = &line[i + 1..];
        let n = rest.chars().take_while(char::is_ascii_digit).count();
        n > 0 && rest[n..].starts_with(':')
    })
}

/// The epilogue every script shares: throw on assertion failure, else `ALL_OK`.
const EPILOGUE: &str = "\nif !empty(v:errors)\n  echo v:errors\n  throw 'intercept-test-fail'\nendif\necho 'ALL_OK'\n";

#[test]
fn intercept_before_and_after_fire_in_order_around_the_call() {
    let src = format!(
        r#"
let g:log = []
function! Foo()
  call add(g:log, 'foo')
  return 42
endfunction
Intercept before Foo {{ call add(g:log, 'before') }}
Intercept after Foo {{ call add(g:log, 'after') }}
let r = Foo()
call assert_equal(['before', 'foo', 'after'], g:log)
call assert_equal(42, r)
{EPILOGUE}"#
    );
    assert_ok(&src);
}

#[test]
fn intercept_around_with_proceed_runs_original_and_returns_its_value() {
    let src = format!(
        r#"
let g:log = []
function! Bar()
  call add(g:log, 'bar')
  return 7
endfunction
Intercept around Bar {{ call add(g:log, 'pre') | let g:rv = intercept_proceed() | call add(g:log, 'post') }}
let r = Bar()
call assert_equal(['pre', 'bar', 'post'], g:log)
call assert_equal(7, r)
call assert_equal(7, g:rv)
{EPILOGUE}"#
    );
    assert_ok(&src);
}

#[test]
fn intercept_around_without_proceed_suppresses_the_original() {
    let src = format!(
        r#"
let g:ran = 0
function! Baz()
  let g:ran = 1
  return 99
endfunction
Intercept around Baz {{ let g:noop = 1 }}
let r = Baz()
call assert_equal(0, g:ran)
call assert_equal(0, r)
{EPILOGUE}"#
    );
    assert_ok(&src);
}

#[test]
fn intercept_builtin_fn_registers_and_exposes_context_vars_to_after_advice() {
    let src = format!(
        r#"
function! Timed()
  return 0
endfunction
let g:id = intercept('after', 'Timed', "let g:seen = g:INTERCEPT_NAME | let g:ms = g:INTERCEPT_MS")
call assert_equal(1, g:id)
call Timed()
call assert_equal('Timed', g:seen)
call assert_true(str2float(g:ms) >= 0.0)
{EPILOGUE}"#
    );
    assert_ok(&src);
}

#[test]
fn intercept_glob_pattern_matches_multiple_functions() {
    let src = format!(
        r#"
let g:hits = []
function! Prep()
  return 0
endfunction
function! Prod()
  return 0
endfunction
Intercept before Pr* {{ call add(g:hits, g:INTERCEPT_NAME) }}
call Prep()
call Prod()
call assert_equal(['Prep', 'Prod'], g:hits)
{EPILOGUE}"#
    );
    assert_ok(&src);
}

#[test]
fn intercept_proceed_builtin_returns_original_value_for_reuse_in_advice() {
    // The around advice can transform the original's return value.
    let src = format!(
        r#"
function! Double()
  return 21
endfunction
Intercept around Double {{ let g:doubled = intercept_proceed() * 2 }}
call Double()
call assert_equal(42, g:doubled)
{EPILOGUE}"#
    );
    assert_ok(&src);
}
