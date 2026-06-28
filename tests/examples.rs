//! Example-script regression gate — runs in CI via `cargo test`.
//!
//! Every `examples/*.vim` is a self-testing script: it exercises a feature,
//! asserts the expected results with the built-in `assert_*` framework, and its
//! epilogue `throw`s (→ non-zero exit) if `v:errors` is non-empty. This harness
//! just runs each script through the built `vimlrs` binary and requires it to
//!   1. exit successfully, and
//!   2. emit no Vim error (`E<num>: …`) on stderr.
//! So an assertion that regresses fails the script, which fails this test.
//!
//! A `tests/fixtures/<name>.in` file, when present, is piped to stdin (used by
//! the interactive example); otherwise stdin is empty (EOF).
//!
//! The binary path comes from `CARGO_BIN_EXE_vimlrs`, which Cargo sets for
//! integration tests — so the build the test exercises is always current.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Sorted list of `examples/*.vim` scripts.
fn example_scripts(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .expect("examples/ dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "vim"))
        .collect();
    v.sort();
    v
}

/// A line is a Vim error if it contains `E<digits>:` (the message format).
fn has_vim_error(line: &str) -> bool {
    line.match_indices('E').any(|(i, _)| {
        let rest = &line[i + 1..];
        let ndigits = rest.chars().take_while(char::is_ascii_digit).count();
        ndigits > 0 && rest[ndigits..].starts_with(':')
    })
}

#[test]
fn examples_self_tests_pass() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bin = env!("CARGO_BIN_EXE_vimlrs");
    let ex_dir = root.join("examples");
    let fixtures = root.join("tests/fixtures");

    let scripts = example_scripts(&ex_dir);
    assert!(!scripts.is_empty(), "no examples/*.vim scripts found");

    let mut failures: Vec<String> = Vec::new();
    for script in &scripts {
        let stem = script.file_stem().unwrap().to_str().unwrap();
        let stdin = match File::open(fixtures.join(format!("{stem}.in"))) {
            Ok(f) => Stdio::from(f),
            Err(_) => Stdio::null(),
        };
        let out = Command::new(bin)
            .arg(script)
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("spawn vimlrs");
        let stderr = String::from_utf8_lossy(&out.stderr);

        if !out.status.success() {
            failures.push(format!(
                "{stem}: exited {:?}\n--- stdout ---\n{}--- stderr ---\n{stderr}",
                out.status.code(),
                String::from_utf8_lossy(&out.stdout),
            ));
        } else if let Some(err) = stderr.lines().find(|l| has_vim_error(l)) {
            failures.push(format!("{stem}: Vim error on stderr: {err}"));
        }
    }

    assert!(
        failures.is_empty(),
        "example self-test failures:\n\n{}",
        failures.join("\n\n")
    );
}
