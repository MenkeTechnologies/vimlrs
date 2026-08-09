//! Replay gate for the script-level differential harness (`scripts/parity.sh`).
//!
//! Each `tests/parity_cases/<name>.vim` is a whole script; the matching
//! `<name>.expected` holds what **real vim** printed for it — the exit status on
//! the first line, then the output (stdout and stderr merged, in the order they
//! were written). Those files are produced only by `bash scripts/parity.sh -r`,
//! which runs vim and nothing else, so a case cannot be made to pass by changing
//! vimlrs — only by matching vim.
//!
//! This test runs the same scripts through the built `viml` and requires the two
//! to be byte-identical. It needs no editor installed, which is what makes the
//! harness's findings safe to gate in CI; `scripts/parity.sh` itself needs vim on
//! PATH and is the development tool that produces and re-verifies the records.
//!
//! To extend it: write a probe, run `bash scripts/parity.sh probe.vim`, confirm
//! the divergence is real, fix it, then drop the probe into `tests/parity_cases/`
//! and record it with `-r`. Never edit an `.expected` by hand.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Cases that record a divergence vimlrs cannot close yet, with the reason.
///
/// This is not a suppression: the `.expected` file still holds vim's real
/// answer, the divergence is still printed by `scripts/parity.sh`, and a case
/// listed here that starts *matching* fails this test — so an entry cannot
/// outlive the gap it names. Every entry must also be an open item in BUGS.md.
/// Empty: every case in the corpus matches vim byte-for-byte. The last entry
/// (`typename_lambda_capture`, BUGS.md R22-O1) was removed when the gap closed —
/// this test reported it as stale on the first run after the fix, which is the
/// whole point of the staleness check.
const KNOWN_OPEN: &[(&str, &str)] = &[];

/// Sorted list of `tests/parity_cases/*.vim`.
fn cases(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .expect("tests/parity_cases dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "vim"))
        .collect();
    v.sort();
    v
}

#[test]
fn parity_cases_match_vim() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("tests/parity_cases");
    let viml = env!("CARGO_BIN_EXE_viml");

    let mut checked = 0;
    let mut failures: Vec<String> = Vec::new();

    for case in cases(&dir) {
        let expected_path = case.with_extension("expected");
        let expected = match fs::read_to_string(&expected_path) {
            Ok(s) => s,
            Err(_) => panic!(
                "{} has no recorded vim output — record it with \
                 `bash scripts/parity.sh -r tests/parity_cases`",
                case.display()
            ),
        };
        checked += 1;

        // stdout and stderr go to ONE file, the way `scripts/parity.sh` captures
        // them with `2>&1`: a script that echoes, errors, then echoes again must
        // record those three in the order they happened. `Command::output()`
        // returns two separate buffers, which would sort every error to the end.
        // `try_clone` dups the descriptor, so both streams share one file offset.
        let sink = tempfile::NamedTempFile::new().expect("capture file");
        let f = sink.reopen().expect("capture handle");
        let status = Command::new(viml)
            .arg(&case)
            .stdout(f.try_clone().expect("dup capture handle"))
            .stderr(f)
            .status()
            .expect("run viml on a parity case");
        let merged = fs::read(sink.path()).expect("read captured output");
        // The record is "status\noutput".
        let got = format!(
            "{}\n{}",
            status.code().unwrap_or(-1),
            String::from_utf8_lossy(&merged)
        );
        // Both sides are compared with trailing newlines removed: vim's message
        // model never writes the final one and viml always does (see
        // `fusevm_bridge::msg_flush_line`), which is a terminal convenience, not
        // a difference in what the script printed.
        let name = case.file_stem().unwrap().to_string_lossy().into_owned();
        let known = KNOWN_OPEN.iter().find(|(n, _)| *n == name);
        let matches = got.trim_end_matches('\n') == expected.trim_end_matches('\n');
        match (known, matches) {
            (None, false) => failures.push(format!(
                "── {name} ── (diverges from vim)\n--- vim (recorded) ---\n{}\n--- viml ---\n{}",
                expected.trim_end_matches('\n'),
                got.trim_end_matches('\n')
            )),
            // The gap named by the entry is gone: delete the entry (and close the
            // BUGS.md item) rather than leave a stale exemption in place.
            (Some((_, why)), true) => failures.push(format!(
                "── {name} ── now MATCHES vim, so its KNOWN_OPEN entry is stale — \
                 remove it from tests/parity_cases.rs.\n  recorded reason: {why}"
            )),
            _ => {}
        }
    }

    assert!(checked > 0, "no parity cases found in {}", dir.display());
    assert!(
        failures.is_empty(),
        "{}/{} parity case(s) diverge from real vim:\n\n{}",
        failures.len(),
        checked,
        failures.join("\n\n")
    );
}
