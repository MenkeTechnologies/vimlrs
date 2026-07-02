//! Drift gate (ported from zshrs's `tests/ported_fn_names_match_c.rs`).
//!
//! Every `fn` defined under `src/ported/` must trace back to upstream Neovim C:
//! its name must appear in `docs/nvim_c_functions.txt` (generated from the
//! vendored `csrc/` by `scripts/gen_c_functions.sh`) OR be a sanctioned
//! exception in `tests/data/fake_fn_allowlist.txt`. Trait-impl methods
//! (`default`, `fmt`, …) and `#[cfg(test)]` functions are exempt.
//!
//! This is the immune system against porting drift: an invented helper name
//! (`make_helper`, `parse_v2`, a bag-of-globals accessor) that doesn't exist in
//! Neovim C and isn't on the allowlist fails the build. Adding a name to the
//! allowlist is a deliberate, reviewed act — never a silent way to pass.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Trait-impl / std method names that legitimately appear without a C origin.
const TRAIT_EXEMPT: &[&str] = &[
    "default",
    "new",
    "fmt",
    "clone",
    "drop",
    "from",
    "into",
    "eq",
    "ne",
    "cmp",
    "partial_cmp",
    "hash",
    "as_ref",
    "as_mut",
    "deref",
    "deref_mut",
    "next",
    "borrow",
    "borrow_mut",
];

fn read_set(path: &Path) -> HashSet<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Collect every function-like C identifier from the vendored `csrc/` tree — the
/// same set `scripts/gen_c_functions.sh` writes to `docs/nvim_c_functions.txt`,
/// computed in-process so the gate needs no generated, git-ignored artifact (it
/// would be empty on a fresh CI checkout). An identifier "appears as a callable"
/// when it is immediately followed (allowing whitespace) by `(`, matching the
/// generator's `\b[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\(` grep.
fn c_names_from_csrc(csrc: &Path) -> HashSet<String> {
    let mut files = Vec::new();
    rs_or_ext_files(csrc, &["c", "h", "lua"], &mut files);
    let mut names = HashSet::new();
    for f in &files {
        let Ok(src) = fs::read_to_string(f) else {
            continue;
        };
        for line in src.lines() {
            let b = line.as_bytes();
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'_' || b[i].is_ascii_alphabetic() {
                    let start = i;
                    while i < b.len() && (b[i] == b'_' || b[i].is_ascii_alphanumeric()) {
                        i += 1;
                    }
                    let ident_end = i;
                    let mut j = i;
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < b.len() && b[j] == b'(' {
                        names.insert(line[start..ident_end].to_string());
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    names
}

/// Recurse `dir`, pushing every file whose extension is in `exts`.
fn rs_or_ext_files(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_or_ext_files(&p, exts, out);
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| exts.contains(&x))
        {
            out.push(p);
        }
    }
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Extract the name from a `fn <name>(` line (handles `pub`, `pub(crate)`,
/// lifetimes like `tv_dict_find<'d>`). Returns `None` if the line isn't a fn def.
/// Return a copy of `line` with the *contents* of string literals, char
/// literals, and a trailing `//` line comment removed, so only real code braces
/// remain. Used for depth tracking; not a full Rust lexer (raw strings and
/// block comments are rare in these files and don't carry stray braces), just
/// enough to keep `{`/`}` counting honest across brace-bearing literals.
fn strip_noncode(line: &str) -> String {
    let b = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if i + 1 < b.len() && b[i + 1] == b'/' => break, // line comment
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'\'' => {
                // char literal 'x' / '\n' / '\'' — skip to the closing quote.
                // (A lone ' e.g. in a lifetime has no close on the same token,
                // so bail the scan at end-of-line if unmatched.)
                let mut j = i + 1;
                while j < b.len() && b[j] != b'\'' {
                    if b[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
                if j < b.len() {
                    i = j; // landed on closing quote; drop the literal body
                } else {
                    out.push('\''); // unmatched (lifetime etc.) — keep as-is
                }
            }
            c => out.push(c as char),
        }
        i += 1;
    }
    out
}

fn fn_name(line: &str) -> Option<String> {
    let t = line.trim_start();
    let rest = t
        .strip_prefix("pub(crate) ")
        .or_else(|| t.strip_prefix("pub "))
        .unwrap_or(t);
    let rest = rest.strip_prefix("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

#[test]
fn ported_fn_names_exist_in_c_or_allowlist() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Derive the legal-name set straight from the vendored C (works on a fresh
    // checkout / in CI); fall back to a pre-generated file only if csrc/ is
    // somehow absent.
    let mut c_names = c_names_from_csrc(&root.join("csrc"));
    if c_names.is_empty() {
        c_names = read_set(&root.join("docs/nvim_c_functions.txt"));
    }
    let allow = read_set(&root.join("tests/data/fake_fn_allowlist.txt"));
    assert!(
        !c_names.is_empty(),
        "no C names found — csrc/ missing and docs/nvim_c_functions.txt empty"
    );

    let mut files = Vec::new();
    rs_files(&root.join("src/ported"), &mut files);
    files.sort();

    let mut violations: Vec<String> = Vec::new();
    for file in &files {
        let src = fs::read_to_string(file).unwrap();
        // Track depth to skip `#[cfg(test)]` / `mod tests { … }` regions.
        let mut depth: i32 = 0;
        let mut test_base: Option<i32> = None;
        let mut pending_test_mod = false;
        let mut prev_was_test_attr = false;

        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.contains("#[cfg(test)]") || trimmed.starts_with("mod tests") {
                pending_test_mod = true;
            }
            let in_test = test_base.is_some();
            if !in_test && !prev_was_test_attr {
                if let Some(name) = fn_name(line) {
                    if !c_names.contains(&name)
                        && !allow.contains(&name)
                        && !TRAIT_EXEMPT.contains(&name.as_str())
                    {
                        let rel = file.strip_prefix(root).unwrap_or(file);
                        violations.push(format!("{}: fn {name}", rel.display()));
                    }
                }
            }
            prev_was_test_attr = trimmed == "#[test]";

            // Update brace depth and test-module bracketing. Count braces on a
            // code-only view of the line: braces inside string/char literals and
            // line comments (e.g. a lambda test string "{a -> a}" or a `'}'` in a
            // comment) must not perturb depth tracking, or the `#[cfg(test)]`
            // region would end early and legitimately test-exempt fns get flagged.
            let code = strip_noncode(line);
            let opens = code.matches('{').count() as i32;
            let closes = code.matches('}').count() as i32;
            if pending_test_mod && opens > 0 {
                test_base = Some(depth);
                pending_test_mod = false;
            }
            depth += opens - closes;
            if let Some(base) = test_base {
                if depth <= base {
                    test_base = None;
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "fn names under src/ported/ with no Neovim C origin and not allowlisted \
         (add a real C-traceable name, or justify in tests/data/fake_fn_allowlist.txt):\n  {}",
        violations.join("\n  ")
    );
}
