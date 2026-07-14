//! AOP function-call intercept / advice machinery — a vimlrs/zshrs-original
//! extension. There is **no Vim counterpart**: Vim has no per-function
//! before/after/around advice. This is a faithful port of zshrs's
//! `src/extensions/intercepts.rs` (which intercepts *commands*); the join point
//! is re-targeted from shell commands onto VimL user-defined function calls
//! (`call Foo(...)` / `Foo(...)`). The engine (`run_intercepts` /
//! `intercept_proceed`) lives in `fusevm_bridge.rs` next to the typval/VM
//! helpers it drives; this module holds the data types, the pattern matcher,
//! the registration store, and the `:Intercept` registry sub-commands.
//!
//! C zsh's closest analog is the function-wrapper hook in `Src/module.c`
//! (`addwrapper()`, used by `zsh/zprof`), but per-function before/after/around
//! AOP intercepts are unique to zshrs — and, now, vimlrs.

use crate::ported::eval::typval::tv_get_string;
use crate::ported::eval::typval_defs_h::{typval_T, typval_vval_union::v_number, varnumber_T};
use std::cell::RefCell;

/// AOP advice type — before, after, or around.
///
/// zshrs-original / vimlrs-original — no Vim (and no C zsh) counterpart.
#[derive(Debug, Clone)]
pub enum AdviceKind {
    /// Run advice code before the function executes.
    Before,
    /// Run advice code after the function executes. `g:INTERCEPT_MS` /
    /// `g:INTERCEPT_US` (elapsed time) are available to the advice.
    After,
    /// Wrap the function. Advice must call `:Intercept proceed` /
    /// `intercept_proceed()` to run the original; otherwise the call is
    /// suppressed (returns 0).
    Around,
}

/// One AOP intercept registered against a function-name pattern.
///
/// zshrs-original / vimlrs-original — no Vim counterpart.
#[derive(Debug, Clone)]
pub struct Intercept {
    /// Pattern to match function names. Supports glob: `Git*`, `_*`, `*`, `all`.
    pub pattern: String,
    /// What kind of advice.
    pub kind: AdviceKind,
    /// VimL code to execute as advice (evaluated in the current interpreter).
    pub code: String,
    /// Unique ID for removal.
    pub id: u32,
}

thread_local! {
    /// The live intercept registrations for this thread. Thread-local to match
    /// vimlrs's other interpreter stores (`FUNCTIONS`, `globvardict`, …), which
    /// are all thread-local `RefCell`s in the single-interpreter-per-thread
    /// model.
    static INTERCEPTS: RefCell<Vec<Intercept>> = const { RefCell::new(Vec::new()) };
}

/// Match an intercept pattern against a function name or a full call string.
/// Supports: `"all"`/`"*"` (match anything), exact match on the name, or a glob
/// (`*`/`?`/`[...]`) matched against either the bare name or the full call.
///
/// Faithful port of zshrs `intercept_matches` (`src/extensions/intercepts.rs`).
pub(crate) fn intercept_matches(pattern: &str, cmd_name: &str, full_cmd: &str) -> bool {
    if pattern == "*" || pattern == "all" {
        return true;
    }
    if pattern == cmd_name {
        return true;
    }
    if pattern.contains('*') || pattern.contains('?') {
        if let Ok(pat) = glob::Pattern::new(pattern) {
            return pat.matches(cmd_name) || pat.matches(full_cmd);
        }
    }
    false
}

/// Snapshot the intercepts whose pattern matches `(cmd_name, full_cmd)`, cloned
/// so the engine can run advice (which may register/remove intercepts) without
/// holding the `INTERCEPTS` borrow.
pub(crate) fn matching(cmd_name: &str, full_cmd: &str) -> Vec<Intercept> {
    INTERCEPTS.with(|v| {
        v.borrow()
            .iter()
            .filter(|i| intercept_matches(&i.pattern, cmd_name, full_cmd))
            .cloned()
            .collect()
    })
}

/// True when no intercepts are registered (fast-path guard for the hot
/// function-call dispatch site).
pub(crate) fn is_empty() -> bool {
    INTERCEPTS.with(|v| v.borrow().is_empty())
}

/// Register a `before|after|around` advice. Returns the new intercept's ID.
/// Mirrors the ID allocation of zshrs `builtin_intercept` (`max(id) + 1`).
pub fn register(kind: AdviceKind, pattern: String, code: String) -> u32 {
    INTERCEPTS.with(|v| {
        let mut v = v.borrow_mut();
        let id = v.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        v.push(Intercept {
            pattern,
            kind,
            code,
            id,
        });
        id
    })
}

fn kind_str(k: &AdviceKind) -> &'static str {
    match k {
        AdviceKind::Before => "before",
        AdviceKind::After => "after",
        AdviceKind::Around => "around",
    }
}

/// `:Intercept list` — print the registered intercepts (user-requested output).
/// Mirrors the table drawn by zshrs `builtin_intercept`'s `list` sub-command.
pub fn list() -> i32 {
    INTERCEPTS.with(|v| {
        let v = v.borrow();
        if v.is_empty() {
            println!("no intercepts registered");
            return 0;
        }
        let bold = |s: &str| format!("\x1b[1m{s}\x1b[0m");
        let cyan = |s: &str| format!("\x1b[36m{s}\x1b[0m");
        println!(
            "{:>4}  {:<8}  {:<20}  {}",
            bold("ID"),
            bold("KIND"),
            bold("PATTERN"),
            bold("CODE")
        );
        for i in v.iter() {
            let code_preview = if i.code.len() > 40 {
                format!("{}...", &i.code[..37])
            } else {
                i.code.clone()
            };
            println!(
                "{:>4}  {:<8}  {:<20}  {}",
                cyan(&i.id.to_string()),
                kind_str(&i.kind),
                i.pattern,
                code_preview
            );
        }
        0
    })
}

/// `:Intercept remove {id}` — drop the intercept with the given ID.
/// Mirrors zshrs `builtin_intercept`'s `remove` sub-command.
pub fn remove(id: u32) -> i32 {
    INTERCEPTS.with(|v| {
        let mut v = v.borrow_mut();
        let before = v.len();
        v.retain(|i| i.id != id);
        if v.len() < before {
            println!("removed intercept {id}");
            0
        } else {
            eprintln!("vimlrs:Intercept:1: no intercept with ID {id}");
            1
        }
    })
}

/// `:Intercept clear` — drop every intercept.
/// Mirrors zshrs `builtin_intercept`'s `clear` sub-command.
pub fn clear() -> i32 {
    INTERCEPTS.with(|v| {
        let mut v = v.borrow_mut();
        let count = v.len();
        v.clear();
        println!("cleared {count} intercepts");
        0
    })
}

// ── VimL/Ex entry points ─────────────────────────────────────────────────────
// These are vimlrs/zshrs-original functions (no Vim counterpart), so they live
// here in the intercept engine rather than under `src/ported/` (whose fns must
// trace to Neovim C per tests/ported_fn_names_match_c.rs).

/// `intercept({kind}, {pattern}, {code})` — register AOP advice on user-function
/// calls; returns the new intercept's numeric ID (or -1 on a bad {kind}). The
/// expression-context form of the `:Intercept before|after|around` sub-command.
/// vimlrs/zshrs-original extension — no Vim counterpart.
pub fn f_intercept(argvars: &[typval_T], rettv: &mut typval_T) {
    let kind = match tv_get_string(&argvars[0]).as_str() {
        "before" => AdviceKind::Before,
        "after" => AdviceKind::After,
        "around" => AdviceKind::Around,
        other => {
            crate::ported::message::semsg(&format!(
                "E475: intercept(): kind must be before|after|around, got '{other}'"
            ));
            rettv.vval = v_number(-1);
            return;
        }
    };
    let id = register(kind, tv_get_string(&argvars[1]), tv_get_string(&argvars[2]));
    rettv.vval = v_number(id as varnumber_T);
}

/// `intercept_proceed()` — from an around advice, run the original intercepted
/// function and return its value. vimlrs/zshrs-original extension.
pub fn f_intercept_proceed(_argvars: &[typval_T], rettv: &mut typval_T) {
    *rettv = crate::fusevm_bridge::intercept_proceed();
}

/// `:Intercept …` handler — the user-facing entry point for the AOP
/// command-intercept extension. Mirrors zshrs's `intercept` builtin sub-commands
/// (`before|after|around <pattern> {code}`, `list`, `remove <id>`, `clear`) and
/// adds `proceed` (the ex-command form of zshrs's `intercept_proceed`, for use
/// inside an around advice). Returns a shell-style status (0 ok) that the Ex
/// dispatcher discards.
pub fn ex_intercept(args: &str) -> i32 {
    use crate::ported::message::{emsg, semsg};
    let args = args.trim();
    if args.is_empty() {
        println!("Usage: :Intercept <before|after|around> <pattern> {{ code }}");
        println!("       :Intercept list | remove <id> | clear | proceed");
        return 0;
    }
    let mut it = args.splitn(2, char::is_whitespace);
    let sub = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("").trim();
    match sub {
        "list" => list(),
        "clear" => clear(),
        "proceed" => {
            // Run the original intercepted function from around advice.
            crate::fusevm_bridge::intercept_proceed();
            0
        }
        "remove" => match rest
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<u32>().ok())
        {
            Some(id) => remove(id),
            None => {
                emsg("E474: :Intercept remove requires a numeric ID");
                1
            }
        },
        "before" | "after" | "around" => {
            let kind = match sub {
                "before" => AdviceKind::Before,
                "after" => AdviceKind::After,
                _ => AdviceKind::Around,
            };
            let mut parts = rest.splitn(2, char::is_whitespace);
            let pattern = parts.next().unwrap_or("").to_string();
            let mut code = parts.next().unwrap_or("").trim().to_string();
            if pattern.is_empty() || code.is_empty() {
                semsg(&format!(
                    "E474: :Intercept {sub} requires <pattern> {{ code }}"
                ));
                return 1;
            }
            // Strip the surrounding `{ … }` braces (mirrors zshrs builtin_intercept).
            if code.starts_with('{') && code.ends_with('}') {
                code = code[1..code.len() - 1].trim().to_string();
            }
            let id = register(kind, pattern, code);
            println!("intercept #{id} registered");
            0
        }
        _ => {
            semsg(&format!("E474: :Intercept: unknown sub-command '{sub}'"));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_matches_anything() {
        assert!(intercept_matches("*", "anything", "anything --here"));
        assert!(intercept_matches("*", "", ""));
    }

    #[test]
    fn all_matches_anything() {
        assert!(intercept_matches("all", "Ls", "Ls -la"));
        assert!(intercept_matches("all", "Git", "Git status"));
    }

    #[test]
    fn exact_match_on_cmd_name() {
        assert!(intercept_matches("Git", "Git", "Git push"));
        assert!(intercept_matches("Ls", "Ls", "Ls -la"));
    }

    #[test]
    fn exact_pattern_does_not_match_different_name() {
        assert!(!intercept_matches("Git", "Svn", "Svn diff"));
        assert!(!intercept_matches("Ls", "Lsof", "Lsof -p 1"));
    }

    #[test]
    fn glob_star_matches_prefix() {
        // "Git *" should match the full call line like "Git push origin".
        assert!(intercept_matches("Git *", "Git", "Git push origin"));
    }

    #[test]
    fn glob_star_underscore_prefix_matches_completion_funcs() {
        // "_*" is the canonical pattern for script-private helper functions.
        assert!(intercept_matches("_*", "_files", "_files"));
        assert!(intercept_matches("_*", "_describe", "_describe"));
    }

    #[test]
    fn glob_star_does_not_match_non_prefix() {
        assert!(!intercept_matches("_*", "files", "files"));
    }

    #[test]
    fn question_mark_glob_matches_single_char() {
        assert!(intercept_matches("L?", "Ls", "Ls"));
        assert!(!intercept_matches("L?", "Lsof", "Lsof"));
    }

    #[test]
    fn unmatched_pattern_without_glob_chars_returns_false() {
        assert!(!intercept_matches("nope", "Git", "Git push"));
    }

    #[test]
    fn invalid_glob_pattern_returns_false() {
        // `[` with no closing bracket contains no `*`/`?`, so glob parsing is
        // never reached; must not panic and must not match.
        assert!(!intercept_matches("[invalid", "Git", "Git push"));
    }

    #[test]
    fn empty_pattern_does_not_match_non_empty_cmd() {
        assert!(!intercept_matches("", "Ls", "Ls -la"));
    }

    #[test]
    fn empty_pattern_matches_empty_cmd_exactly() {
        // Falls through to the `pattern == cmd_name` check.
        assert!(intercept_matches("", "", ""));
    }

    #[test]
    fn advice_kind_variants_round_trip_clone() {
        let b = AdviceKind::Before;
        let a = AdviceKind::After;
        let r = AdviceKind::Around;
        assert!(matches!(b.clone(), AdviceKind::Before));
        assert!(matches!(a.clone(), AdviceKind::After));
        assert!(matches!(r.clone(), AdviceKind::Around));
    }

    #[test]
    fn intercept_struct_clone_preserves_fields() {
        let i = Intercept {
            pattern: "Git *".into(),
            kind: AdviceKind::Before,
            code: "echo 'before'".into(),
            id: 42,
        };
        let c = i.clone();
        assert_eq!(c.pattern, "Git *");
        assert!(matches!(c.kind, AdviceKind::Before));
        assert_eq!(c.code, "echo 'before'");
        assert_eq!(c.id, 42);
    }

    #[test]
    fn register_allocates_increasing_ids_then_remove_clear() {
        // Thread-local store: this test owns the store on its own test thread.
        clear();
        let id1 = register(AdviceKind::Before, "Foo".into(), "echo 1".into());
        let id2 = register(AdviceKind::After, "Foo".into(), "echo 2".into());
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert!(!is_empty());
        assert_eq!(matching("Foo", "Foo").len(), 2);
        assert_eq!(matching("Bar", "Bar").len(), 0);
        assert_eq!(remove(id1), 0);
        assert_eq!(matching("Foo", "Foo").len(), 1);
        assert_eq!(remove(999), 1); // unknown ID
        clear();
        assert!(is_empty());
    }
}
