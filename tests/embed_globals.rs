//! Tests for `set_global_string`, the entry point an embedder uses to hand a
//! script data (a buffer's text, a selection) without building a `:let` line.
//!
//! The point of the API is that the value never passes through the parser, so
//! nothing about it needs escaping.

use vimlrs::fusevm_bridge::{capture_begin, capture_take, set_global_string};

/// Run `src`, returning its captured `:echo` output.
fn echo(src: &str) -> String {
    capture_begin();
    vimlrs::eval_source(src).expect("script failed");
    capture_take().trim_end_matches('\n').to_string()
}

/// A bound global reads as an ordinary `g:` variable.
#[test]
fn a_bound_global_is_readable() {
    set_global_string("stdin", "hello");
    assert_eq!(echo("echo toupper(g:stdin)"), "HELLO");
}

/// It is a real string, so string builtins act on it.
#[test]
fn a_bound_global_is_a_string() {
    set_global_string("stdin", "abc");
    assert_eq!(echo("echo type(g:stdin) . ':' . strlen(g:stdin)"), "1:3");
}

/// Text that would break a generated `:let` line survives untouched: quotes,
/// backslashes and a `|` (VimL's command separator) are data, not syntax.
#[test]
fn hostile_text_is_data_not_syntax() {
    let hostile = "a \"quoted\" \\ back | bar";
    set_global_string("stdin", hostile);
    assert_eq!(echo("echo g:stdin"), hostile);
}

/// Rebinding replaces the value rather than accumulating.
#[test]
fn rebinding_replaces_the_value() {
    set_global_string("stdin", "first");
    set_global_string("stdin", "second");
    assert_eq!(echo("echo g:stdin"), "second");
}
