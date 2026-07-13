//! Replay gate for the differential fuzzer's findings (`tests/data/fuzz_corpus.txt`).
//!
//! Each corpus line pairs an expression with the result **real Vim produces** —
//! recorded by running it through Vim 9.2 *and* Neovim 0.12 and keeping only the
//! cases where the two agreed (see the file's header). So this test asserts
//! vimlrs against Vim's behavior, not against its own: a case cannot be made to
//! pass by changing vimlrs, only by matching Vim.
//!
//! It runs in-process and needs no editor installed, which is what makes the
//! fuzzer's findings safe to gate in CI — `cargo run --bin fuzz-parity` needs
//! `nvim` + `vim` on PATH and is a development tool, this is not.
//!
//! To extend it: run the fuzzer, confirm a divergence is real, fix it, then add
//! the repro here with the oracle's answer (`docs/FUZZING.md` has the recording
//! recipe). Never edit an expectation to match vimlrs.

use vimlrs::ported::eval::encode::encode_tv2string;
use vimlrs::ported::message::{capture_errors_begin, capture_errors_take};

/// `E<digits>` prefix of a Vim error message — the stable part of the contract
/// (the prose is not).
fn enumber(msg: &str) -> String {
    let b = msg.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'E' && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
            let d: String = msg[i + 1..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if msg[i + 1 + d.len()..].starts_with(':') {
                return format!("E{d}");
            }
        }
    }
    "E?".into()
}

/// Evaluate one expression the way the corpus records it: `string()` of the
/// value, or `!E<num>` for the error it raises.
fn eval_as_corpus(expr: &str) -> String {
    capture_errors_begin();
    let out = vimlrs::eval_expr(expr);
    let errs = capture_errors_take();
    match out {
        // An expression can both produce a (recovered, empty) value and raise an
        // error; the error is the observable outcome, so it wins — the oracle
        // recorded it the same way.
        Ok(v) if errs.is_empty() => encode_tv2string(&v),
        Ok(_) => format!("!{}", enumber(&errs[0])),
        Err(e) => format!("!{}", enumber(&e.to_string())),
    }
}

#[test]
fn fuzz_corpus_matches_vim() {
    let text = include_str!("data/fuzz_corpus.txt");
    let mut cases = 0;
    let mut failures: Vec<String> = Vec::new();

    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((expr, want)) = line.split_once('\t') else {
            panic!("corpus line is not `expr<TAB>expected`: {line}");
        };
        cases += 1;
        let got = eval_as_corpus(expr);
        if got != want {
            failures.push(format!(
                "  {expr}\n    expected (Vim): {want}\n    got  (vimlrs): {got}"
            ));
        }
    }

    assert!(cases > 0, "fuzz corpus is empty");
    assert!(
        failures.is_empty(),
        "{} of {cases} fuzz-corpus cases diverge from real Vim:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
