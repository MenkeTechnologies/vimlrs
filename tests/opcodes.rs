//! The builtin-id invariant.
//!
//! Every `VIML_*` constant in `src/fusevm_bridge.rs` is a key into fusevm's
//! builtin table, and registration is a plain overwrite:
//!
//! ```text
//! pub fn register_builtin(&mut self, id: u16, handler: BuiltinHandler) {
//!     let idx = id as usize;
//!     if idx >= self.builtin_table.len() {
//!         self.builtin_table.resize(idx + 1, None);
//!     }
//!     self.builtin_table[idx] = Some(handler);
//! }
//! ```
//!
//! (fusevm 0.17.0, `src/vm.rs:912`.) Two constants sharing a number therefore
//! does not fail to build, does not warn and does not panic — the later
//! `register_builtin` silently replaces the earlier handler and the shadowed
//! construct starts doing something else entirely. The ids are hand-assigned, so
//! two independent edits can pick the same free number, and the two files merge
//! without a conflict marker.
//!
//! This has bitten across the fusevm frontends: scalars shipped `MAKE_ORDERING`
//! and `MAKE_QUEUE` both at 754, and phplang shipped `INDEX_ISSET` at 105 where
//! `LIST_ELEM_GET` already sat, so `isset($a['k'])` quietly dispatched to the
//! destructuring read. In this crate the same shape is already recorded in the
//! source: the comment above `VIML_CMP_IC_OFFSET` (`src/fusevm_bridge.rs`)
//! documents two ignore-case offsets that were shipped and then withdrawn
//! because the *derived* comparison ids they produced landed on
//! `VIML_INDEX`/`VIML_SLICE`/`VIML_ECHO` and on the `VIML_FN_GETCHAR` cluster —
//! so `==?` dispatched to those instead of comparing.
//!
//! Everything below is read out of the source rather than from a hand-kept
//! list, because a hand-kept list is the thing that goes stale and stops
//! catching this. The comparison ids in particular are obtained by calling the
//! real [`cmp_id`], not by re-deriving its arithmetic here — a copy of the
//! arithmetic would drift away from the function it is meant to police.

use vimlrs::fusevm_bridge::cmp_id;
use vimlrs::viml_lexer::{CaseFlag, CmpOp};

const BRIDGE_SRC: &str = include_str!("../src/fusevm_bridge.rs");
const LEXER_SRC: &str = include_str!("../src/viml_lexer.rs");

/// Lowest id VimL may claim. The builtin-id space is shared across the fusevm
/// frontends — `src/fusevm_bridge.rs` heads the block with "AWK uses 1000–2999,
/// VimL 3000+" — and fusevm's own shell builtins occupy the bottom of the table
/// up to `BUILTIN_MAX = 280` (fusevm 0.17.0, `src/shell_builtins.rs:420`). A
/// VimL id below the floor would shadow one of those instead of colliding with
/// a sibling, which no duplicate check would see.
const VIML_ID_FLOOR: u16 = 3000;

/// `(name, number)` for every `pub const VIML_NAME: u16 = N;` in the bridge.
fn builtin_id_constants() -> Vec<(String, u16)> {
    let mut out = Vec::new();
    for line in BRIDGE_SRC.lines() {
        let Some(rest) = line.strip_prefix("pub const VIML_") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(": u16 = ") else {
            continue;
        };
        let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(n) = digits.parse::<u16>() {
            out.push((format!("VIML_{name}"), n));
        }
    }
    out
}

/// Every `VIML_*` name passed to `vm.register_builtin(...)`, in source order and
/// with repeats kept — a name registered twice is itself a silent shadow.
fn registered_names() -> Vec<String> {
    let mut out = Vec::new();
    for line in BRIDGE_SRC.lines() {
        let Some((_, rest)) = line.split_once("register_builtin(") else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.starts_with("VIML_") {
            out.push(name);
        }
    }
    out
}

/// Count the variants of `pub enum NAME` in the lexer source.
///
/// The `ALL_*` arrays below have to enumerate the variants by hand (Rust has no
/// stable way to iterate an enum), and a hand-written array is exactly what goes
/// stale. Pinning its length against the enum as written means a new `CmpOp`
/// fails this file until its ids are checked too.
fn variant_count(enum_name: &str) -> usize {
    let head = format!("pub enum {enum_name} {{");
    let body = BRIDGE_SRC
        .split_once(&head)
        .or_else(|| LEXER_SRC.split_once(&head))
        .unwrap_or_else(|| panic!("`{head}` not found in the lexer or bridge source"))
        .1;
    let mut n = 0;
    for line in body.lines() {
        let line = line.trim();
        if line == "}" {
            return n;
        }
        // A variant line: `Ident,` — doc comments and attributes are skipped.
        let ident = line.trim_end_matches(',');
        if !ident.is_empty()
            && ident.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && line.ends_with(',')
        {
            n += 1;
        }
    }
    panic!("`{head}` never closed");
}

const ALL_CMP_OPS: [CmpOp; 10] = [
    CmpOp::Equal,
    CmpOp::NotEqual,
    CmpOp::Match,
    CmpOp::NoMatch,
    CmpOp::Greater,
    CmpOp::GreaterEqual,
    CmpOp::Less,
    CmpOp::LessEqual,
    CmpOp::Is,
    CmpOp::IsNot,
];

const ALL_CASE_FLAGS: [CaseFlag; 3] =
    [CaseFlag::Default, CaseFlag::MatchCase, CaseFlag::IgnoreCase];

/// `(label, id)` for every id [`cmp_id`] can return.
fn derived_cmp_ids() -> Vec<(String, u16)> {
    let mut out = Vec::new();
    for op in ALL_CMP_OPS {
        for case in ALL_CASE_FLAGS {
            out.push((format!("cmp_id({op:?}, {case:?})"), cmp_id(op, case)));
        }
    }
    out
}

/// Group `(name, id)` pairs by id, keeping only the ids more than one name
/// claims, rendered for an assertion message.
fn clashes(pairs: &[(String, u16)]) -> Vec<String> {
    let mut by_id: Vec<(u16, Vec<&str>)> = Vec::new();
    for (name, id) in pairs {
        match by_id.iter_mut().find(|(n, _)| n == id) {
            Some((_, names)) => names.push(name),
            None => by_id.push((*id, vec![name])),
        }
    }
    by_id.sort_by_key(|(id, _)| *id);
    by_id
        .iter()
        .filter(|(_, names)| names.len() > 1)
        .map(|(id, names)| format!("{id} => {}", names.join(", ")))
        .collect()
}

#[test]
fn every_builtin_id_is_used_by_exactly_one_constant() {
    let ids = builtin_id_constants();
    // A parse that finds nothing would pass vacuously, which is worse than no
    // test at all.
    assert!(
        ids.len() > 400,
        "expected to read the whole id block, found {} constants",
        ids.len()
    );
    let clashes = clashes(&ids);
    assert!(
        clashes.is_empty(),
        "builtin ids claimed by more than one constant — the later \
         register_builtin silently replaces the earlier handler: {}",
        clashes.join("; ")
    );
}

#[test]
fn no_derived_comparison_id_collides_with_a_registered_constant() {
    assert_eq!(
        ALL_CMP_OPS.len(),
        variant_count("CmpOp"),
        "CmpOp gained or lost a variant — update ALL_CMP_OPS so its ids are checked"
    );
    assert_eq!(
        ALL_CASE_FLAGS.len(),
        variant_count("CaseFlag"),
        "CaseFlag gained or lost a variant — update ALL_CASE_FLAGS so its ids are checked"
    );

    let registered: Vec<String> = registered_names();
    // Only ids that actually carry a handler can shadow one another; a constant
    // that is merely a family base (see the exact-set test below) is consumed by
    // `cmp_id` rather than registered, and is expected to equal a derived id.
    let mut pairs: Vec<(String, u16)> = builtin_id_constants()
        .into_iter()
        .filter(|(name, _)| registered.iter().any(|r| r == name))
        .collect();
    pairs.extend(derived_cmp_ids());

    let clashes = clashes(&pairs);
    assert!(
        clashes.is_empty(),
        "a comparison id derived by cmp_id() lands on another registered \
         builtin, so that operator dispatches to it instead of comparing: {}",
        clashes.join("; ")
    );
}

#[test]
fn no_builtin_id_reaches_below_the_viml_block() {
    let low: Vec<String> = builtin_id_constants()
        .into_iter()
        .chain(derived_cmp_ids())
        .filter(|(_, id)| *id < VIML_ID_FLOOR)
        .map(|(name, id)| format!("{name} = {id}"))
        .collect();
    assert!(
        low.is_empty(),
        "builtin ids below the VimL block start ({VIML_ID_FLOOR}) shadow \
         fusevm's own builtins or another frontend's: {}",
        low.join(", ")
    );
}

#[test]
fn no_constant_is_registered_twice() {
    let mut seen: Vec<&str> = Vec::new();
    let mut twice: Vec<&str> = Vec::new();
    let names = registered_names();
    assert!(
        names.len() > 400,
        "expected to read the whole install() block, found {} registrations",
        names.len()
    );
    for name in &names {
        if seen.contains(&name.as_str()) {
            twice.push(name);
        } else {
            seen.push(name);
        }
    }
    assert!(
        twice.is_empty(),
        "registered more than once — only the last handler survives: {}",
        twice.join(", ")
    );
}

#[test]
fn the_only_unregistered_constant_is_the_comparison_family_base() {
    let registered = registered_names();
    let orphans: Vec<String> = builtin_id_constants()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !registered.iter().any(|r| r == name))
        .collect();
    // `VIML_CMP_BASE` is not registered under its own name: `cmp_id` adds the
    // per-operator offset and `register_cmp_handlers` registers the results. Any
    // OTHER unregistered constant is a builtin that was declared, is emitted by
    // the compiler, and has no handler — `CallBuiltin` on it is a runtime miss.
    assert_eq!(
        orphans,
        vec!["VIML_CMP_BASE".to_string()],
        "declared builtin ids with no registered handler"
    );
}
