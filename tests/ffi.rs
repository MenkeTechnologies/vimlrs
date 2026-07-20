//! End-to-end inline Rust FFI: a `rust { ... }` block is desugared, compiled to
//! a cdylib via `rustc`, `dlopen`ed, and its exports called from VimL. Driven
//! through the built `viml` binary (`CARGO_BIN_EXE_viml`), so the FFI's
//! `dlopen`ed libraries never load into the test process. Requires `rustc` on
//! PATH (always present in a Rust CI); skips cleanly otherwise so a
//! toolchain-less environment never reports a false failure.

use std::io::Write;
use std::process::Command;

use tempfile::NamedTempFile;

fn rustc_available() -> bool {
    Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `src` as a `.vim` script through the built `viml` binary; return
/// `(stdout, stderr, success)`.
fn run_viml(src: &str) -> (String, String, bool) {
    let mut f = NamedTempFile::with_suffix(".vim").expect("temp .vim");
    f.write_all(src.as_bytes()).expect("write script");
    let path = f.path().to_path_buf();
    let out = Command::new(env!("CARGO_BIN_EXE_viml"))
        .arg(&path)
        .output()
        .expect("spawn viml");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

#[test]
fn rust_block_export_returns_42() {
    if !rustc_available() {
        eprintln!("skipping FFI test: rustc not on PATH");
        return;
    }
    // The headline case: an exported `add` shadows Vim's list `add()` builtin
    // and `add(21, 21)` returns 42.
    let src = "rust { pub extern \"C\" fn add(a: i64, b: i64) -> i64 { a + b } }\necho add(21, 21)\n";
    let (stdout, stderr, ok) = run_viml(src);
    assert!(ok, "viml failed; stderr: {stderr}");
    assert_eq!(stdout.trim(), "42", "stdout was {stdout:?}; stderr {stderr:?}");
}

#[test]
fn rust_block_all_v1_signatures() {
    if !rustc_available() {
        return;
    }
    // int-arity, float-arity, and string→int marshalling, with a leading legacy
    // `"` comment (the swallow hazard) directly above the block.
    let src = r#"" a legacy comment mentioning rust { and a stray ; above the block
rust {
    pub extern "C" fn ffi_addi(a: i64, b: i64) -> i64 { a + b }
    pub extern "C" fn ffi_mulf(x: f64, y: f64, z: f64) -> f64 { x * y * z }
    pub extern "C" fn ffi_slen(s: *const c_char) -> i64 {
        unsafe { CStr::from_ptr(s).to_bytes().len() as i64 }
    }
}
echo ffi_addi(21, 21)
echo ffi_mulf(1.5, 2.0, 3.0)
echo ffi_slen("hello world")
"#;
    let (stdout, stderr, ok) = run_viml(src);
    assert!(ok, "viml failed; stderr: {stderr}");
    let lines: Vec<&str> = stdout.lines().map(str::trim).collect();
    assert_eq!(lines, ["42", "9.0", "11"], "stdout {stdout:?}; stderr {stderr:?}");
}

#[test]
fn rust_block_call_statement_form() {
    if !rustc_available() {
        return;
    }
    // Invoked via `:call` (statement position) rather than in an expression.
    let src = "rust { pub extern \"C\" fn ffi_note(a: i64) -> i64 { a } }\ncall ffi_note(7)\necho ffi_note(7)\n";
    let (stdout, stderr, ok) = run_viml(src);
    assert!(ok, "viml failed; stderr: {stderr}");
    assert_eq!(stdout.trim(), "7", "stdout {stdout:?}; stderr {stderr:?}");
}

#[test]
fn rust_block_with_no_exports_errors() {
    if !rustc_available() {
        return;
    }
    // A block with no `pub extern "C" fn` is a hard error — v1 requires at least
    // one exported function. The error text reaches stderr.
    let src = "rust { fn helper() -> i64 { 1 } }\necho 1\n";
    let (_stdout, stderr, _ok) = run_viml(src);
    assert!(stderr.contains("rust FFI"), "unexpected stderr: {stderr}");
}
