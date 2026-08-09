//! Example-script regression gate — runs in CI via `cargo test`.
//!
//! Every `examples/*.vim` is a self-testing script: it exercises a feature,
//! asserts the expected results with the built-in `assert_*` framework, and its
//! epilogue `throw`s (→ non-zero exit) if `v:errors` is non-empty. This harness
//! just runs each script through the built `viml` binary and requires it to
//!   1. exit successfully, and
//!   2. emit no Vim error (`E<num>: …`) on stderr.
//!
//! So an assertion that regresses fails the script, which fails this test.
//!
//! A `tests/fixtures/<name>.in` file, when present, is piped to stdin (used by
//! the interactive example); otherwise stdin is empty (EOF).
//!
//! The binary path comes from `CARGO_BIN_EXE_viml`, which Cargo sets for
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

/// Examples whose self-tests record a divergence vimlrs cannot close yet, with
/// the reason. Same contract as `tests/parity_cases.rs`'s `KNOWN_OPEN`.
///
/// This is NOT a suppression, and it is strictly stricter than what stood here
/// before. Until round 25 this gate reported ZERO failures across the whole
/// corpus — not because there were none, but because `install()` re-ran
/// `evalvars_init()` on every nested VM, which emptied `v:errors`. Any
/// `execute()`, `assert_fails()` or user-function call discarded every assertion
/// failure recorded before it, so a script's epilogue read an empty `v:errors`
/// and printed "all assertions passed". Fixing that (BUGS.md R25-2) made 8
/// real, long-standing failures visible for the first time; three were wrong
/// expectations in the example and were corrected against both oracles, and
/// these five are genuine open gaps.
///
/// An entry that starts PASSING fails this test, so it cannot outlive the gap it
/// names, and every entry must also be an open item in BUGS.md. A script NOT
/// listed here must still pass outright — a new failure is a hard failure.
const KNOWN_OPEN: &[(&str, &str)] = &[
    (
        "builtin_arity",
        "R25-O1: assert_fails() does not observe a builtin arity error (E119/E118) \
         raised from a `call` statement in this file's context, so four checks \
         report 'command did not fail'. Both oracles pass this script.",
    ),
    (
        "testing",
        "R25-O1: same assert_fails() detection gap, for a user function's \
         argument-count error (`call ParseKV('nope')`). Both oracles pass.",
    ),
    (
        "json",
        "R25-O2: json_encode() Dict key ORDER. The assertion hardcodes vim 9.2's \
         order, which vim reproduces and this port does not; nvim 0.12.4 emits \
         this port's order but with a space after ':' and ','. Oracle-dependent \
         — the assertion needs rewriting to be order-independent.",
    ),
    (
        "map_commands",
        "R25-O3: len(maplist()) is 6/5 here against the script's 5/4. Both \
         oracles disagree with the script AND with each other (nvim 103/102, \
         vim 12/11 — they count their own default mappings), so the expected \
         value has to be made relative rather than absolute.",
    ),
    (
        "vim9_script_scope",
        "R25-O4: a vim9 script-scope counter reads 0 where the script expects 3. \
         Both oracles pass this script, so this is a real vim9 scoping gap.",
    ),
];

#[test]
fn examples_self_tests_pass() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bin = env!("CARGO_BIN_EXE_viml");
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
            .expect("spawn viml");
        let stderr = String::from_utf8_lossy(&out.stderr);

        let problem = if !out.status.success() {
            Some(format!(
                "{stem}: exited {:?}\n--- stdout ---\n{}--- stderr ---\n{stderr}",
                out.status.code(),
                String::from_utf8_lossy(&out.stdout),
            ))
        } else {
            stderr
                .lines()
                .find(|l| has_vim_error(l))
                .map(|err| format!("{stem}: Vim error on stderr: {err}"))
        };

        match (KNOWN_OPEN.iter().find(|(n, _)| *n == stem), problem) {
            // Not exempt and it failed: a hard failure, as before.
            (None, Some(msg)) => failures.push(msg),
            // The gap named by the entry is gone: delete the entry (and close the
            // BUGS.md item) rather than leave a stale exemption in place.
            (Some((_, why)), None) => failures.push(format!(
                "{stem}: now PASSES, so its KNOWN_OPEN entry is stale — remove it \
                 from tests/examples.rs.\n  recorded reason: {why}"
            )),
            _ => {}
        }
    }

    assert!(
        failures.is_empty(),
        "example self-test failures:\n\n{}",
        failures.join("\n\n")
    );
}
