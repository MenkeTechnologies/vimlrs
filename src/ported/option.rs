//! Port of `src/nvim/option.c` (subset) — the option table, `&opt` access, and
//! the `:set` command parser (`do_set`).
//!
//! Neovim's option machinery is large (hundreds of options, per-buffer/window
//! scopes, side effects). This ports the common global boolean/number options
//! plus the `do_set` argument grammar (`set opt`, `set noopt`, `set opt!`,
//! `set inv opt`, `set opt=val`, `set opt?`); the value store is a thread-local
//! map seeded with Vim's defaults. String options and per-buffer scopes follow
//! with the editor integration.
#![allow(non_snake_case, non_upper_case_globals)]

use std::cell::RefCell;
use std::collections::HashMap;

use crate::ported::eval::typval::tv_get_bool;
use crate::ported::eval::typval_defs_h::{typval_T, varnumber_T};

/// Option kind, for parsing `:set` values.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    Number,
    /// String options (`'shell'`, `'filetype'`, …) arrive with editor
    /// integration; the parse path already handles them.
    #[allow(dead_code)]
    String,
}

/// `(canonical name, abbreviation, kind, default)` rows of the supported option
/// table — the subset of `options[]` (`option.c`) ported so far.
const OPTIONS: &[(&str, &str, Kind, varnumber_T)] = &[
    ("ignorecase", "ic", Kind::Bool, 0),
    ("smartcase", "scs", Kind::Bool, 0),
    ("magic", "magic", Kind::Bool, 1),
    ("expandtab", "et", Kind::Bool, 0),
    ("number", "nu", Kind::Bool, 0),
    ("relativenumber", "rnu", Kind::Bool, 0),
    ("wrap", "wrap", Kind::Bool, 1),
    ("hlsearch", "hls", Kind::Bool, 0),
    ("incsearch", "is", Kind::Bool, 0),
    ("autoindent", "ai", Kind::Bool, 0),
    ("tabstop", "ts", Kind::Number, 8),
    ("shiftwidth", "sw", Kind::Number, 8),
    ("softtabstop", "sts", Kind::Number, 0),
    ("textwidth", "tw", Kind::Number, 0),
    ("scrolloff", "so", Kind::Number, 0),
];

thread_local! {
    /// Current option values, keyed by canonical name. Lazily seeded from the
    /// table defaults on first access.
    static option_values: RefCell<HashMap<String, typval_T>> = RefCell::new(HashMap::new());
}

/// Port of `findoption()` (`option.c`) — resolve an option name or abbreviation
/// to its `OPTIONS` row.
fn findoption(name: &str) -> Option<&'static (&'static str, &'static str, Kind, varnumber_T)> {
    OPTIONS
        .iter()
        .find(|(n, abbr, _, _)| *n == name || *abbr == name)
}

/// Port of `set_option_value()` (`option.c`) reduced — store option `canon`'s
/// value.
fn set_option_value(canon: &str, tv: typval_T) {
    option_values.with(|m| {
        m.borrow_mut().insert(canon.to_string(), tv);
    });
}

/// Port of `get_option_value()` (`option.c`) reduced — the value of `&name` (or
/// its abbreviation). Unknown options yield "" (the empty string).
pub fn get_option_value(name: &str) -> typval_T {
    let Some((canon, _, kind, default)) = findoption(name) else {
        return typval_T::from(String::new());
    };
    option_values.with(|m| {
        m.borrow()
            .get(*canon)
            .cloned()
            .unwrap_or_else(|| match kind {
                Kind::String => typval_T::from(String::new()),
                _ => typval_T::from(*default),
            })
    })
}

thread_local! {
    /// Host hook fired with the raw `:set` argument string whenever `:set` runs,
    /// so an embedding editor (zemacs) can mirror the option onto its own live
    /// config. EXTENSION — no `csrc/` counterpart; the analogue of Vim's
    /// option-change side-effect callbacks (`did_set_*`). The installer lives in
    /// the crate-root carve-out [`crate::fusevm_bridge::install_set_hook`] (net-new
    /// synthesis does not belong under `src/ported/`); unset by default (no-op).
    pub static SET_HOST_HOOK: std::cell::RefCell<Option<Box<dyn Fn(&str)>>> =
        const { std::cell::RefCell::new(None) };
}

/// Port of `do_set()` (`option.c`) — parse and apply a `:set` argument string:
/// `set opt` / `set noopt` / `set opt!` / `set invopt` / `set opt=val` /
/// `set opt:val` / `set opt?` (whitespace-separated, multiple per line).
pub fn do_set(args: &str) {
    // Mirror the whole `:set` line to the host editor first (if a hook is
    // installed), then keep vimlrs' own option table in sync below so `&opt`
    // reads inside vimscript still see the value.
    SET_HOST_HOOK.with(|h| {
        if let Some(f) = h.borrow().as_ref() {
            f(args);
        }
    });
    for part in args.split_whitespace() {
        // `opt=val` / `opt:val`.
        if let Some((name, val)) = part.split_once(['=', ':']) {
            if let Some((canon, _, kind, _)) = findoption(name) {
                let tv = match kind {
                    Kind::String => typval_T::from(val.to_string()),
                    _ => typval_T::from(val.trim().parse().unwrap_or(0)),
                };
                set_option_value(canon, tv);
            }
            continue;
        }
        // `opt!` (toggle a bool) / `opt?` (query — no-op here).
        if let Some(name) = part.strip_suffix('!') {
            if let Some((canon, _, Kind::Bool, _)) = findoption(name) {
                let cur = tv_get_bool(&get_option_value(canon)) != 0;
                set_option_value(canon, typval_T::from(varnumber_T::from(!cur)));
            }
            continue;
        }
        if part.ends_with('?') {
            continue; // query form: no terminal output in this subset
        }
        // `noopt` / `invopt` (bool off / invert).
        if let Some(name) = part.strip_prefix("no") {
            if let Some((canon, _, Kind::Bool, _)) = findoption(name) {
                set_option_value(canon, typval_T::from(0));
                continue;
            }
        }
        if let Some(name) = part.strip_prefix("inv") {
            if let Some((canon, _, Kind::Bool, _)) = findoption(name) {
                let cur = tv_get_bool(&get_option_value(canon)) != 0;
                set_option_value(canon, typval_T::from(varnumber_T::from(!cur)));
                continue;
            }
        }
        // Bare `opt` — turn a boolean on (number/string forms are queries).
        if let Some((canon, _, Kind::Bool, _)) = findoption(part) {
            set_option_value(canon, typval_T::from(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_host_hook_fires_with_raw_args() {
        use std::cell::RefCell;
        thread_local! { static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }
        super::SET_HOST_HOOK.with(|h| {
            *h.borrow_mut() = Some(Box::new(|a: &str| {
                SEEN.with(|s| s.borrow_mut().push(a.to_string()))
            }));
        });
        super::do_set("number tw=80");
        SEEN.with(|s| assert_eq!(s.borrow().as_slice(), &["number tw=80".to_string()]));
        // and vimlrs' own option table still tracks it (dual-write):
        assert!(super::findoption("tw").is_some());
    }

    #[test]
    fn set_and_get_bool_and_number() {
        let ic = || tv_get_bool(&get_option_value("ignorecase")) != 0;
        do_set("ignorecase");
        assert!(ic());
        do_set("noic"); // abbreviation + no-prefix
        assert!(!ic());
        do_set("ic!"); // toggle
        assert!(ic());
        do_set("tabstop=4");
        assert_eq!(
            crate::ported::eval::typval::tv_get_number_chk(&get_option_value("ts"), None),
            4
        );
    }
}
