//! VimL wiring for inline Rust FFI (`rust { ... }` blocks).
//!
//! The heavy lifting lives in fusevm: [`fusevm::RustSugar`] scans and rewrites
//! the block at the source level, and [`fusevm::ffi`] compiles/loads/marshals
//! it. This module supplies the VimL-flavoured [`fusevm::RustSugar`] config, the
//! desugar entry the parser calls before lexing, and the set of exported names
//! collected from every block so the compiler can route a call by that name
//! through the runtime FFI path even when it collides with a Vim builtin. The
//! emitted `__rust_compile(...)` call and every exported bareword are resolved
//! in [`crate::fusevm_bridge`]'s `b_call_user`.
//!
//! ## The legacy-`"` comment hazard, and how it is handled
//!
//! Legacy Vimscript uses `"` to introduce a comment; the same byte also opens a
//! double-quoted string. The generic desugar scanner treats `"`/`'`/`` ` `` as
//! string delimiters that span until the matching quote — so a *single* leading
//! `"` comment (`" a note`, the overwhelmingly common form) would look like an
//! UNTERMINATED string and swallow the rest of the file looking for a closing
//! `"`, corrupting any real `rust { ... }` block below it (whose body contains
//! `pub extern "C"`). Setting `line_comments: &[]` — the naive legacy choice —
//! is therefore broken.
//!
//! The fix is to tell the scanner that `"` *is* a line-comment introducer
//! ([`SUGAR`] below): its line-comment branch copies the skipped run verbatim
//! and stops at the newline, which is exactly legacy `"`-comment semantics. A
//! `"`-string on a code line is likewise copied verbatim to end-of-line and
//! never mis-scanned for `rust {`. `#` is included too so a Vim9 `#` comment
//! before a block behaves identically. `rust {` is only ever recognized at a
//! real statement boundary (`newline_boundary: true`), and it is never valid
//! Vimscript outside an FFI block, so a real block is only matched when
//! intended and comment/string text is never falsely desugared. The tests below
//! pin every branch of this.

use std::cell::RefCell;

use fusevm::RustSugar;
use rustc_hash::FxHashSet;

/// Emit the VimL statement a `rust { ... }` block desugars to: a `:call` of the
/// `__rust_compile` builtin carrying the base64-encoded block body and its line.
/// A bare `Fn(...)` is not a statement in Vimscript — `:call` is how a function
/// is invoked for its side effect — so the replacement is spelled `call …`.
/// The base64 alphabet (`A–Za–z0–9+/=`) contains no `"`, so embedding it in a
/// double-quoted string literal is always safe.
fn emit(b64: &str, line: usize) -> String {
    format!("call __rust_compile(\"{b64}\", {line})")
}

/// VimL desugar config. `"` and `#` are both treated as line-comment
/// introducers (see the module docs): copying such runs verbatim to
/// end-of-line matches legacy `"`-comment and Vim9 `#`-comment semantics and
/// stops the scanner from mis-reading a `"`-comment as a multi-line string.
/// `newline_boundary` is `true` because Vimscript is line-oriented, so `rust {`
/// is recognized only at the start of a statement.
pub const SUGAR: RustSugar = RustSugar {
    keyword: "rust",
    line_comments: &["\"", "#"],
    block_comment: None,
    newline_boundary: true,
    emit,
};

thread_local! {
    /// Names exported by every `rust { ... }` block seen this session. The
    /// compiler consults [`is_ffi_export`] to route a call to such a name
    /// through the runtime FFI path (VIML_CALL_USER) instead of a Vim builtin,
    /// so an exported `add`/`len`/etc. shadows the builtin of the same name —
    /// mirroring how a PHP `rust` export shadows the PHP standard library.
    /// User `:function`s still win (they are resolved before FFI at runtime).
    static FFI_EXPORTS: RefCell<FxHashSet<String>> = RefCell::new(FxHashSet::default());
}

/// Rewrite every top-level `rust { ... }` block into a `call __rust_compile(...)`
/// statement before lexing, and record the block's exported function names so
/// the compiler can override a same-named builtin. No-op when the source has no
/// `rust` token (single substring scan on the fast path).
pub fn desugar(src: &str) -> String {
    for name in collect_export_names(src) {
        FFI_EXPORTS.with(|e| {
            e.borrow_mut().insert(name);
        });
    }
    SUGAR.desugar(src)
}

/// Whether `name` is exported by a `rust { ... }` block seen this session. The
/// compiler calls this before builtin resolution so an FFI export shadows a Vim
/// builtin of the same name.
pub fn is_ffi_export(name: &str) -> bool {
    FFI_EXPORTS.with(|e| e.borrow().contains(name))
}

/// Collect the names of `extern "C" fn <name>` declarations inside every
/// `rust { ... }` block of `src`. Scans only within block bodies so a stray
/// `extern "C" fn foo` in an ordinary Vimscript string can never be mistaken
/// for an FFI export.
fn collect_export_names(src: &str) -> Vec<String> {
    let bytes = src.as_bytes();
    let mut names = Vec::new();
    let mut search_from = 0usize;
    // Find each `rust {` at a plausible boundary, take its balanced `{...}`
    // body, and scan the body for exported function names.
    while let Some(rel) = src[search_from..].find("rust") {
        let kw = search_from + rel;
        // `rust` must be a whole word: preceded by a boundary and not part of a
        // longer identifier (`trust`, `rusty`).
        let before_ok =
            kw == 0 || !matches!(bytes[kw - 1], b'_' | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z');
        let after = kw + 4;
        let after_ok = after >= bytes.len()
            || !matches!(bytes[after], b'_' | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z');
        search_from = kw + 4;
        if !(before_ok && after_ok) {
            continue;
        }
        // Skip whitespace to the opening brace.
        let mut j = after;
        while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'\r' | b'\n') {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'{' {
            continue;
        }
        // Balance braces to find the block end (good enough for name scanning —
        // fusevm's scanner does the authoritative brace/string handling).
        let body_start = j + 1;
        let mut depth = 1i32;
        let mut k = body_start;
        while k < bytes.len() && depth > 0 {
            match bytes[k] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            k += 1;
        }
        let body_end = if depth == 0 { k - 1 } else { bytes.len() };
        scan_extern_c_fns(&src[body_start..body_end], &mut names);
        search_from = body_end;
    }
    names
}

/// Push every `extern "C" fn <name>` function name found in `body` onto `out`.
fn scan_extern_c_fns(body: &str, out: &mut Vec<String>) {
    let bytes = body.as_bytes();
    let mut i = 0usize;
    while let Some(rel) = body[i..].find("extern") {
        let mut p = i + rel + "extern".len();
        i = p;
        // whitespace, then `"C"`.
        let skip_ws = |p: &mut usize| {
            while *p < bytes.len() && matches!(bytes[*p], b' ' | b'\t' | b'\r' | b'\n') {
                *p += 1;
            }
        };
        skip_ws(&mut p);
        if !body[p..].starts_with("\"C\"") {
            continue;
        }
        p += 3;
        skip_ws(&mut p);
        if !body[p..].starts_with("fn") {
            continue;
        }
        p += 2;
        skip_ws(&mut p);
        // Read the identifier.
        let start = p;
        while p < bytes.len() && matches!(bytes[p], b'_' | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z')
        {
            p += 1;
        }
        if p > start {
            out.push(body[start..p].to_string());
        }
        i = p;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desugars_block_becomes_call() {
        let src =
            "rust { pub extern \"C\" fn add(a: i64, b: i64) -> i64 { a + b } }\necho add(2, 3)\n";
        let out = desugar(src);
        assert!(
            out.contains("call __rust_compile("),
            "no builtin call: {out}"
        );
        assert!(!out.contains("pub extern"), "Rust body leaked: {out}");
        assert!(out.contains("echo add(2, 3)"));
    }

    #[test]
    fn collects_exported_names() {
        let src = "rust {\n  pub extern \"C\" fn add(a: i64, b: i64) -> i64 { a + b }\n  extern \"C\" fn mul2(x: f64, y: f64) -> f64 { x * y }\n}\n";
        let _ = desugar(src);
        assert!(is_ffi_export("add"), "add not collected");
        assert!(is_ffi_export("mul2"), "mul2 not collected");
        assert!(!is_ffi_export("not_exported"));
    }

    #[test]
    fn leaves_ordinary_viml_untouched() {
        let src = "let x = strlen(\"hi\")\necho x\n";
        assert_eq!(desugar(src), src);
    }

    // ── legacy-`"` comment hazard: none of these must falsely desugar ──

    #[test]
    fn legacy_single_quote_comment_does_not_swallow_following_block() {
        // The classic hazard: a single leading-`"` comment (unterminated as a
        // string) directly above a real block. With `"` as a line comment the
        // comment ends at the newline and the block below desugars correctly.
        let src = "\" a note about the ffi\nrust { pub extern \"C\" fn f() -> i64 { 1 } }\n";
        let out = desugar(src);
        assert!(
            out.contains("call __rust_compile("),
            "block not desugared: {out}"
        );
        assert!(
            out.starts_with("\" a note about the ffi\n"),
            "comment mangled: {out}"
        );
        assert!(!out.contains("pub extern"), "Rust body leaked: {out}");
    }

    #[test]
    fn rust_brace_inside_legacy_comment_not_desugared() {
        let src = "\" this mentions rust { and a stray ; too\necho 1\n";
        assert_eq!(desugar(src), src, "comment text was falsely desugared");
    }

    #[test]
    fn rust_brace_inside_string_not_desugared() {
        let src = "let s = \"rust { not a block }\"\necho s\n";
        assert_eq!(desugar(src), src);
    }

    #[test]
    fn vim9_hash_comment_before_block_desugars() {
        let src =
            "# a vim9 comment mentioning rust {\nrust { pub extern \"C\" fn g() -> i64 { 2 } }\n";
        let out = desugar(src);
        assert!(
            out.contains("call __rust_compile("),
            "block not desugared: {out}"
        );
        assert!(
            out.starts_with("# a vim9 comment"),
            "comment mangled: {out}"
        );
    }

    #[test]
    fn word_boundary_rust_not_matched() {
        // `trust`/`rusty` must not trigger the keyword.
        let src = "let trusty = 1\nlet rusty = 2\n";
        assert_eq!(desugar(src), src);
    }

    #[test]
    fn extern_c_fn_in_viml_string_not_collected() {
        // A `extern "C" fn ghost` inside a Vimscript string is not in a block,
        // so it must not be registered as an export.
        let src = "let doc = \"extern \\\"C\\\" fn ghost() {}\"\necho doc\n";
        let _ = desugar(src);
        assert!(!is_ffi_export("ghost"), "string content falsely collected");
    }
}
