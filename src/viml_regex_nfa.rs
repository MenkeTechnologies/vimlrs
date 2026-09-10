//! Port of Vim's NFA regexp engine (`regexp_nfa.c`, vim 9.2 patch 1000).
//!
//! Vim ships TWO regexp engines and `'regexpengine'` defaults to `0`
//! ("automatic"): `nfa_regcomp()` is tried first and the old backtracker
//! (`regexp_bt.c`) is used only when the NFA compile bails out. Several
//! observable answers are properties of *which* engine produced them, and no
//! amount of patching a backtracker reproduces them:
//!
//! * Alternation priority. `matchstr('aa', '^\|a*')` is `'aa'` in vim. It is
//!   neither "first branch wins" (that would be `''`) nor "longest wins"
//!   (`matchstr('ab', 'a\|ab')` is `'a'`). It falls out of the thread ordering
//!   in [`Matcher::addstate`]: `^` compiles to a state transition that is added
//!   to the *current* list, so the `a*` thread reaches `NFA_MATCH` at a later
//!   character while the anchored thread is still sitting at column 0.
//! * `E363` (`'maxmempattern'`). A pattern whose thread list outgrows the
//!   memory budget raises it; a backtracker with the same answer set would
//!   either return a value or spin.
//!
//! What is ported here, and where each piece comes from:
//!
//! | Rust | c: `regexp_nfa.c` |
//! |---|---|
//! | [`Post::emit_alt`] / [`Post::emit_branch`] / [`Post::emit_piece`] / [`Post::emit_atom`] | `nfa_reg` / `nfa_regbranch` / `nfa_regconcat` / `nfa_regpiece` / `nfa_regatom` |
//! | [`post2nfa`] with [`Frag`] / [`patch`] / [`append`] | `post2nfa`, `frag`, `patch`, `append`, `alloc_state` |
//! | [`Matcher::addstate`] / [`Matcher::addstate_here`] | `addstate` / `addstate_here` |
//! | [`Matcher::regmatch`] | `nfa_regmatch` |
//! | [`Matcher::recursive_regmatch`] | `recursive_regmatch` |
//! | [`Matcher::regtry`] | `nfa_regtry` |
//! | [`nfa_postprocess`] / [`match_follows`] / [`failure_chance`] / [`nfa_max_width`] | same names |
//! | [`nfa_get_reganch`] / [`nfa_get_regstart`] / [`nfa_get_match_text`] / [`skip_to_start`] / [`find_match_text`] | same names |
//! | [`exec`] | `nfa_regexec_both` |
//!
//! The last two rows are the search fast paths: they let a pattern whose match
//! must begin with a known character skip whole runs of the subject instead of
//! starting a thread list at every position, and they let a pattern that is one
//! literal string answer without the simulation at all. They are what makes the
//! NFA engine competitive with the backtracker it replaced — see `R42-O1` in
//! `BUGS.md` for the measurement — and they change no answer.
//!
//! Two deliberate, documented departures — both because this crate matches ONE
//! string rather than a buffer, which is the `REG_MULTI == FALSE` half of every
//! `if (REG_MULTI)` in the C:
//!
//! 1. **Positions are `char` indices, not `char_u *`.** `rex.line` is index 0 of
//!    the subject slice and `rex.input` is an index into it. Everything the C
//!    measures in bytes (`bytelen` for a backreference, the `\@<=` distance,
//!    `NFA_SKIP`'s count) is measured in characters here, consistently. The one
//!    place a byte count is still a byte count is `NFA_COL` (`\%23c`), which is
//!    a byte column in vim and is computed as one.
//! 2. **`NFA_CLASS_OBJ`** (this port's only added opcode, placed below
//!    `NFA_SPLIT` so it cannot collide) replaces the `NFA_START_COLL` …
//!    `NFA_END_COLL` chain and the `NFA_DIGIT`/`NFA_KWORD`/… class atoms with a
//!    single state carrying an index into [`Prog::classes`]. The parser in
//!    `viml_regex.rs` already folds both spellings into one [`Class`], whose
//!    membership rules were pinned against vim in earlier rounds; re-deriving
//!    them from `check_char_class` would relitigate settled behaviour. It is
//!    behaviourally identical to the chain it replaces: a collection consumes
//!    exactly one character and adds `out` with `add_off = clen` when a member
//!    matches, which is what this state does.
//!
//! Not ported, so [`Prog::compile`] returns `None` and the caller falls back to
//! the backtracker exactly as vim falls back to `regexp_bt.c`: `\z(`/`\z1`
//! (`FEAT_SYN_HL` external submatches, which this crate has no consumer for),
//! `NFA_COMPOSING` (a pattern whose literal carries its own combining marks),
//! and the multi-line atoms `NFA_LNUM`/`NFA_VCOL`/`NFA_MARK`/`NFA_VISUAL`.

use super::{Atom, Branch, Class, Cmp, LookOp, Node, INF};
use crate::ported::strings::utf_iscomposing;

// ── opcodes (c: the anonymous enum at regexp_nfa.c:36) ──
//
// The values matter: the C does arithmetic on them (`NFA_MOPEN + parno`,
// `mclose = *p + NSUBEXP`, `state->c - NFA_MCLOSE`), so they are transcribed
// with their exact numbering rather than renumbered.

pub const NFA_SPLIT: i32 = -1024;
pub const NFA_MATCH: i32 = -1023;
pub const NFA_EMPTY: i32 = -1022;
pub const NFA_CONCAT: i32 = -1014;
pub const NFA_OR: i32 = -1013;
pub const NFA_STAR: i32 = -1012;
pub const NFA_STAR_NONGREEDY: i32 = -1011;
pub const NFA_QUEST: i32 = -1010;
pub const NFA_QUEST_NONGREEDY: i32 = -1009;
pub const NFA_BOL: i32 = -1008;
pub const NFA_EOL: i32 = -1007;
pub const NFA_BOW: i32 = -1006;
pub const NFA_EOW: i32 = -1005;
pub const NFA_BOF: i32 = -1004;
pub const NFA_EOF: i32 = -1003;
pub const NFA_ZSTART: i32 = -1001;
pub const NFA_ZEND: i32 = -1000;
pub const NFA_NOPEN: i32 = -999;
pub const NFA_NCLOSE: i32 = -998;
pub const NFA_START_INVISIBLE: i32 = -997;
pub const NFA_START_INVISIBLE_FIRST: i32 = -996;
pub const NFA_START_INVISIBLE_NEG: i32 = -995;
pub const NFA_START_INVISIBLE_NEG_FIRST: i32 = -994;
pub const NFA_START_INVISIBLE_BEFORE: i32 = -993;
pub const NFA_START_INVISIBLE_BEFORE_FIRST: i32 = -992;
pub const NFA_START_INVISIBLE_BEFORE_NEG: i32 = -991;
pub const NFA_START_INVISIBLE_BEFORE_NEG_FIRST: i32 = -990;
pub const NFA_START_PATTERN: i32 = -989;
pub const NFA_END_INVISIBLE: i32 = -988;
pub const NFA_END_INVISIBLE_NEG: i32 = -987;
pub const NFA_END_PATTERN: i32 = -986;
pub const NFA_ANY_COMPOSING: i32 = -983;
pub const NFA_OPT_CHARS: i32 = -982;
pub const NFA_PREV_ATOM_NO_WIDTH: i32 = -981;
pub const NFA_PREV_ATOM_NO_WIDTH_NEG: i32 = -980;
pub const NFA_PREV_ATOM_JUST_BEFORE: i32 = -979;
pub const NFA_PREV_ATOM_JUST_BEFORE_NEG: i32 = -978;
pub const NFA_PREV_ATOM_LIKE_PATTERN: i32 = -977;
pub const NFA_BACKREF1: i32 = -976;
pub const NFA_BACKREF9: i32 = -968;
pub const NFA_SKIP: i32 = -958;
pub const NFA_MOPEN: i32 = -957;
pub const NFA_MOPEN9: i32 = -948;
pub const NFA_MCLOSE: i32 = -947;
pub const NFA_MCLOSE9: i32 = -938;
pub const NFA_ANY: i32 = -917;
pub const NFA_CURSOR: i32 = -855;
pub const NFA_COL: i32 = -851;
pub const NFA_COL_GT: i32 = -850;
pub const NFA_COL_LT: i32 = -849;

/// This port's one added opcode — see the module docs. Placed below
/// `NFA_SPLIT` (the lowest C value) so it can never collide with one.
pub const NFA_CLASS_OBJ: i32 = -2048;

/// c: `regexp.h:22` `#define NSUBEXP 10` — `\0` plus `\1`..`\9`.
const NSUBEXP: usize = 10;

/// c: `regexp.h:33` `#define NFA_MAX_STATES 100000`. Reaching it makes
/// `nfa_regmatch()` return `NFA_TOO_EXPENSIVE`, which under the automatic
/// engine means "retry with the backtracker".
const NFA_MAX_STATES: u32 = 100_000;

/// c: `addstate()` `if (++depth >= 5000 …) return NULL`.
const MAX_ADDSTATE_DEPTH: usize = 5000;

/// c: `#define ADDSTATE_HERE_OFFSET 10`.
const ADDSTATE_HERE_OFFSET: i32 = 10;

/// `sizeof(nfa_thread_T)` on a 64-bit build with `FEAT_SYN_HL`, which is what
/// the `'maxmempattern'` check in `addstate()` divides the budget by:
/// `nfa_state_T *state` 8 + `int count` 4 (+4 pad) + `nfa_pim_T pim` 544 +
/// `regsubs_T subs` 512. `nfa_pim_T` is `int result` 4 (+4) + `state` 8 +
/// `regsubs_T subs` 512 + `union end` 16; `regsubs_T` is two `regsub_T`, each
/// `int in_use` 4 (+4 pad) + `struct multipos multi[10]` 240 (the larger arm of
/// the union) + `colnr_T orig_start_col` 4 (+4 pad) = 256.
const SIZEOF_NFA_THREAD_T: usize = 1072;

/// c: `p_mmp` — the `'maxmempattern'` option, in kilobytes. Vim's default.
const P_MMP: usize = 1000;

/// The `nfa_regmatch()` outcome. c: `TRUE` / `FALSE` / `NFA_TOO_EXPENSIVE`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MatchResult {
    No,
    Yes,
    TooExpensive,
}

/// The error a run can raise. c: the two `emsg()` calls in `addstate()` and
/// `addstate_here()`.
pub const E_MAXMEMPATTERN: &str = "E363: Pattern uses more memory than 'maxmempattern'";

// ── compiled program ──

/// c: `nfa_state_T`. `out`/`out1` are indices into [`Prog::states`]; `-1` is
/// the C's `NULL`.
#[derive(Clone, Debug)]
struct State {
    c: i32,
    out: i32,
    out1: i32,
    val: i32,
}

/// A compiled NFA. c: `nfa_regprog_T`.
pub struct Prog {
    states: Vec<State>,
    start: usize,
    nsubexp: usize,
    has_zend: bool,
    has_backref: bool,
    /// Side table for [`NFA_CLASS_OBJ`]; a state's `val` indexes it.
    classes: Vec<Class>,
    /// c: `nfa_regprog_T.reganch` — every path out of the start state begins
    /// with `^`/`\%^`, so a match can only begin at column zero.
    reganch: bool,
    /// c: `nfa_regprog_T.regstart` — the character every match must begin
    /// with, or `NUL` (`0`) when there is none. Held as the C's `int` because
    /// [`nfa_get_regstart`] reads it straight out of `nfa_state_T.c`, where a
    /// value `> 0` is a literal character and everything else is an opcode.
    regstart: i32,
    /// c: `nfa_regprog_T.match_text` — set only when the whole program is one
    /// literal string, and then holding that string *minus* its first
    /// character, which is [`Prog::regstart`].
    match_text: Option<Vec<char>>,
}

// ── postfix form (c: re2post / nfa_reg / nfa_regbranch / nfa_regconcat / nfa_regpiece / nfa_regatom) ──

/// The postfix emitter. `viml_regex.rs` has already done the *parsing* — and
/// with it every diagnostic `nfa_regatom()` raises — so what is ported here is
/// the emission ORDER, which is what decides the shape of the NFA and therefore
/// the thread priority.
struct Post {
    post: Vec<i32>,
    classes: Vec<Class>,
    has_zend: bool,
    has_backref: bool,
    /// c: `wants_nfa`, set by `[[:upper:]]` and `[[:lower:]]` (the only two
    /// places, `regexp_nfa.c:1858` and `:1871`). It suppresses the `\{n,m}`
    /// bail-out below, because those classes do not work in the BT engine.
    wants_nfa: bool,
}

impl Post {
    fn emit(&mut self, c: i32) {
        self.post.push(c);
    }

    /// c: `nfa_reg()` — `branch` (`\|` `branch`)*, without the trailing
    /// `NFA_MOPEN`/`NFA_ZOPEN` the caller emits.
    fn emit_alt(&mut self, branches: &[Branch]) -> bool {
        let Some((first, rest)) = branches.split_first() else {
            // An alternation with no branches cannot come out of the parser;
            // treat it as the empty pattern rather than underflowing the stack.
            self.emit(NFA_EMPTY);
            return true;
        };
        if !self.emit_branch(first) {
            return false;
        }
        for b in rest {
            if !self.emit_branch(b) {
                return false;
            }
            self.emit(NFA_OR);
        }
        true
    }

    /// c: `nfa_regconcat()` plus `nfa_regbranch()`'s "if a branch is empty,
    /// emit one node for it".
    ///
    /// `\&` needs nothing here: the parser already desugars `a\&b` into
    /// `\%(a\)\@=b`, and that desugaring emits the same postfix
    /// `NFA_NOPEN NFA_PREV_ATOM_NO_WIDTH … NFA_CONCAT` that `nfa_regbranch()`
    /// emits by hand for a `\&`.
    fn emit_branch(&mut self, b: &Branch) -> bool {
        let start = self.post.len();
        let mut first = true;
        for a in b {
            if !self.emit_piece(a) {
                return false;
            }
            if first {
                first = false;
            } else {
                self.emit(NFA_CONCAT);
            }
        }
        if self.post.len() == start {
            self.emit(NFA_EMPTY);
        }
        true
    }

    /// c: `nfa_regpiece()` — an atom plus its multi.
    ///
    /// The C reaches `\+` through its own `case Magic('+')` and `\{1,}` through
    /// the brace loop, but both emit `<atom> <atom> NFA_STAR NFA_CONCAT`, so the
    /// AST not distinguishing them costs nothing. `\{n,m}` is expanded by
    /// emitting the atom `maxval` times, exactly as the C re-parses it that many
    /// times from a saved parse state.
    fn emit_piece(&mut self, a: &Atom) -> bool {
        let my_post_start = self.post.len();
        if !self.emit_atom(&a.node) {
            return false;
        }
        let (min, max, greedy) = (a.min, a.max, a.greedy);
        if min == 1 && max == 1 {
            return true;
        }
        // <atom>{0,inf}, <atom>{0,} and <atom>{} are equivalent to <atom>*
        if min == 0 && max == INF {
            self.emit(if greedy { NFA_STAR } else { NFA_STAR_NONGREEDY });
            return true;
        }
        // Special case: x{0} or x{-0}
        if max == 0 {
            self.post.truncate(my_post_start);
            self.emit(NFA_EMPTY);
            return true;
        }
        if min == 0 && max == 1 {
            self.emit(if greedy {
                NFA_QUEST
            } else {
                NFA_QUEST_NONGREEDY
            });
            return true;
        }
        // c: "The engine is very inefficient (uses too many states) when the
        // maximum is much larger than the minimum … Bail out if we can use the
        // other engine". `RE_AUTO` is always set here: this crate has no
        // `'regexpengine'` option, so it is always the automatic engine.
        if max != INF && (max > 500 || max > min + 200) && min < 200 && !self.wants_nfa {
            return false;
        }
        self.post.truncate(my_post_start);
        let quest = if greedy {
            NFA_QUEST
        } else {
            NFA_QUEST_NONGREEDY
        };
        let mut i = 0u32;
        while i < max {
            let old_post_pos = self.post.len();
            if !self.emit_atom(&a.node) {
                return false;
            }
            // after "minval" times, atoms are optional
            if i + 1 > min {
                if max == INF {
                    self.emit(if greedy { NFA_STAR } else { NFA_STAR_NONGREEDY });
                } else {
                    self.emit(quest);
                }
            }
            if old_post_pos != my_post_start {
                self.emit(NFA_CONCAT);
            }
            if i + 1 > min && max == INF {
                break;
            }
            i += 1;
        }
        true
    }

    /// c: `nfa_regatom()`, emission only.
    fn emit_atom(&mut self, n: &Node) -> bool {
        match n {
            Node::Lit(c) => {
                // A NUL literal would be indistinguishable from end-of-input.
                if *c == '\0' {
                    return false;
                }
                self.emit(*c as i32);
            }
            Node::Any => self.emit(NFA_ANY),
            Node::Bol => self.emit(NFA_BOL),
            Node::Eol => self.emit(NFA_EOL),
            Node::WordB(start) => self.emit(if *start { NFA_BOW } else { NFA_EOW }),
            Node::MatchStart => self.emit(NFA_ZSTART),
            Node::MatchEnd => {
                self.has_zend = true;
                self.emit(NFA_ZEND);
            }
            Node::BackRef(k) => {
                if *k == 0 || *k > 9 {
                    return false;
                }
                self.has_backref = true;
                self.emit(NFA_BACKREF1 + (*k as i32 - 1));
            }
            // c: `case '['` under `Magic('%')` — the atoms are emitted first,
            // then `NFA_OPT_CHARS`, their count, and an `NFA_NOPEN` so that a
            // `\%[abc]*` cannot match the empty string without bound.
            Node::OptSeq(nodes) => {
                if nodes.is_empty() {
                    return false;
                }
                for k in nodes {
                    if !self.emit_atom(k) {
                        return false;
                    }
                }
                self.emit(NFA_OPT_CHARS);
                self.emit(nodes.len() as i32);
                self.emit(NFA_NOPEN);
            }
            Node::Class(cl) => {
                if cl.wants_nfa() {
                    self.wants_nfa = true;
                }
                let idx = self.classes.len() as i32;
                self.classes.push(cl.clone());
                self.emit(NFA_CLASS_OBJ);
                self.emit(idx);
            }
            Node::Group(branches, cap) => {
                if !self.emit_alt(branches) {
                    return false;
                }
                match cap {
                    Some(i) if *i >= 1 && *i <= 9 => self.emit(NFA_MOPEN + *i as i32),
                    Some(_) => return false,
                    None => self.emit(NFA_NOPEN),
                }
            }
            Node::Look(atom, op) => {
                if !self.emit_piece(atom) {
                    return false;
                }
                match op {
                    LookOp::Ahead(true) => self.emit(NFA_PREV_ATOM_NO_WIDTH),
                    LookOp::Ahead(false) => self.emit(NFA_PREV_ATOM_NO_WIDTH_NEG),
                    LookOp::Behind(positive, n) => {
                        self.emit(if *positive {
                            NFA_PREV_ATOM_JUST_BEFORE
                        } else {
                            NFA_PREV_ATOM_JUST_BEFORE_NEG
                        });
                        // c: `EMIT(c2)` where `c2 = getdecchrs()`, which is 0
                        // when no count was written.
                        self.emit(i32::try_from(n.unwrap_or(0)).unwrap_or(0));
                    }
                    LookOp::Atomic => self.emit(NFA_PREV_ATOM_LIKE_PATTERN),
                }
            }
            Node::AnyComposing => self.emit(NFA_ANY_COMPOSING),
            Node::FileEnd(start) => self.emit(if *start { NFA_BOF } else { NFA_EOF }),
            Node::Col(cmp, n) => {
                self.emit(match cmp {
                    Cmp::Eq => NFA_COL,
                    Cmp::Gt => NFA_COL_GT,
                    Cmp::Lt => NFA_COL_LT,
                });
                self.emit(i32::try_from(*n).unwrap_or(i32::MAX));
            }
            // `\%#` / `\%V` / `\%1l` / `\%'m` all parse to a node that can never
            // match a string. `NFA_CURSOR` is the faithful spelling: its
            // runtime test is `rex.reg_win != NULL && …`, and a string match
            // has no window, so it never matches.
            Node::CheckPos(target) if *target == usize::MAX => self.emit(NFA_CURSOR),
            // A match-time-only node; the parser never puts one in a pattern.
            Node::CheckPos(_) => return false,
        }
        true
    }
}

// ── post2nfa (c: post2nfa / frag / patch / append / list1 / alloc_state) ──

/// c: `Ptrlist` — in the C this is a linked list threaded through the not-yet-
/// filled-in `out` pointers themselves; here it is the list of the slots to be
/// filled, as `(state index, which of out/out1)`.
type PtrList = Vec<(usize, u8)>;

/// c: `Frag_T`.
#[derive(Clone, Default)]
struct Frag {
    start: usize,
    out: PtrList,
}

struct Builder {
    states: Vec<State>,
}

impl Builder {
    /// c: `alloc_state()`.
    fn alloc_state(&mut self, c: i32, out: i32, out1: i32) -> usize {
        self.states.push(State {
            c,
            out,
            out1,
            val: 0,
        });
        self.states.len() - 1
    }

    /// c: `patch()`.
    fn patch(&mut self, l: &PtrList, s: i32) {
        for &(idx, which) in l {
            if which == 0 {
                self.states[idx].out = s;
            } else {
                self.states[idx].out1 = s;
            }
        }
    }
}

/// c: `append()`.
fn append(mut l1: PtrList, l2: PtrList) -> PtrList {
    l1.extend(l2);
    l1
}

/// c: `list1()`.
fn list1(idx: usize, which: u8) -> PtrList {
    vec![(idx, which)]
}

/// c: `post2nfa()`. The `nfa_calc_size` pass exists only to size a C
/// allocation, so only the `FALSE` (build) pass is ported.
fn post2nfa(postfix: &[i32]) -> Option<(Vec<State>, usize)> {
    let mut b = Builder { states: Vec::new() };
    let mut stack: Vec<Frag> = Vec::new();
    let mut p = 0usize;
    while p < postfix.len() {
        let op = postfix[p];
        match op {
            NFA_CONCAT => {
                let e2 = stack.pop()?;
                let e1 = stack.pop()?;
                b.patch(&e1.out, e2.start as i32);
                stack.push(Frag {
                    start: e1.start,
                    out: e2.out,
                });
            }
            NFA_OR => {
                let e2 = stack.pop()?;
                let e1 = stack.pop()?;
                let s = b.alloc_state(NFA_SPLIT, e1.start as i32, e2.start as i32);
                stack.push(Frag {
                    start: s,
                    out: append(e1.out, e2.out),
                });
            }
            NFA_STAR => {
                // Zero or more, prefer more
                let e = stack.pop()?;
                let s = b.alloc_state(NFA_SPLIT, e.start as i32, -1);
                b.patch(&e.out, s as i32);
                stack.push(Frag {
                    start: s,
                    out: list1(s, 1),
                });
            }
            NFA_STAR_NONGREEDY => {
                // Zero or more, prefer zero
                let e = stack.pop()?;
                let s = b.alloc_state(NFA_SPLIT, -1, e.start as i32);
                b.patch(&e.out, s as i32);
                stack.push(Frag {
                    start: s,
                    out: list1(s, 0),
                });
            }
            NFA_QUEST => {
                // one or zero atoms => greedy match
                let e = stack.pop()?;
                let s = b.alloc_state(NFA_SPLIT, e.start as i32, -1);
                stack.push(Frag {
                    start: s,
                    out: append(e.out, list1(s, 1)),
                });
            }
            NFA_QUEST_NONGREEDY => {
                // zero or one atoms => non-greedy match
                let e = stack.pop()?;
                let s = b.alloc_state(NFA_SPLIT, -1, e.start as i32);
                stack.push(Frag {
                    start: s,
                    out: append(e.out, list1(s, 0)),
                });
            }
            NFA_EMPTY => {
                // 0-length, used in a repetition with max/min count of 0
                let s = b.alloc_state(NFA_EMPTY, -1, -1);
                stack.push(Frag {
                    start: s,
                    out: list1(s, 0),
                });
            }
            NFA_OPT_CHARS => {
                // \%[abc] implemented as a chain of NFA_SPLITs, one per
                // character, each of whose out1 skips the rest.
                p += 1;
                let mut n = *postfix.get(p)?;
                let mut s = 0usize;
                let mut e1: Option<Frag> = None;
                let mut s1: i32 = -1;
                while n > 0 {
                    n -= 1;
                    let e = stack.pop()?;
                    s = b.alloc_state(NFA_SPLIT, e.start as i32, -1);
                    let first = e1.is_none();
                    if first {
                        e1 = Some(e.clone());
                    }
                    b.patch(&e.out, s1);
                    let acc = e1.take().unwrap_or_default();
                    e1 = Some(Frag {
                        start: acc.start,
                        out: append(acc.out, list1(s, 1)),
                    });
                    s1 = s as i32;
                }
                let acc = e1?;
                stack.push(Frag {
                    start: s,
                    out: acc.out,
                });
            }
            NFA_PREV_ATOM_NO_WIDTH
            | NFA_PREV_ATOM_NO_WIDTH_NEG
            | NFA_PREV_ATOM_JUST_BEFORE
            | NFA_PREV_ATOM_JUST_BEFORE_NEG
            | NFA_PREV_ATOM_LIKE_PATTERN => {
                let before = op == NFA_PREV_ATOM_JUST_BEFORE || op == NFA_PREV_ATOM_JUST_BEFORE_NEG;
                let pattern = op == NFA_PREV_ATOM_LIKE_PATTERN;
                let (start_state, end_state) = match op {
                    NFA_PREV_ATOM_NO_WIDTH => (NFA_START_INVISIBLE, NFA_END_INVISIBLE),
                    NFA_PREV_ATOM_NO_WIDTH_NEG => (NFA_START_INVISIBLE_NEG, NFA_END_INVISIBLE_NEG),
                    NFA_PREV_ATOM_JUST_BEFORE => (NFA_START_INVISIBLE_BEFORE, NFA_END_INVISIBLE),
                    NFA_PREV_ATOM_JUST_BEFORE_NEG => {
                        (NFA_START_INVISIBLE_BEFORE_NEG, NFA_END_INVISIBLE_NEG)
                    }
                    _ => (NFA_START_PATTERN, NFA_END_PATTERN),
                };
                let mut n = 0i32;
                if before {
                    p += 1;
                    n = *postfix.get(p)?;
                }
                let e = stack.pop()?;
                let s1 = b.alloc_state(end_state, -1, -1);
                let s = b.alloc_state(start_state, e.start as i32, s1 as i32);
                if pattern {
                    // NFA_ZEND -> NFA_END_PATTERN -> NFA_SKIP -> what follows.
                    let skip = b.alloc_state(NFA_SKIP, -1, -1);
                    let zend = b.alloc_state(NFA_ZEND, s1 as i32, -1);
                    b.states[s1].out = skip as i32;
                    b.patch(&e.out, zend as i32);
                    stack.push(Frag {
                        start: s,
                        out: list1(skip, 0),
                    });
                } else {
                    b.patch(&e.out, s1 as i32);
                    stack.push(Frag {
                        start: s,
                        out: list1(s1, 0),
                    });
                    if before {
                        if n <= 0 {
                            // See if we can guess the maximum width, it avoids
                            // a lot of pointless tries.
                            n = nfa_max_width(&b.states, e.start as i32, 0);
                        }
                        b.states[s].val = n;
                    }
                }
            }
            NFA_MOPEN..=NFA_MOPEN9 | NFA_NOPEN => {
                let mopen = op;
                let mclose = if op == NFA_NOPEN {
                    NFA_NCLOSE
                } else {
                    op + NSUBEXP as i32
                };
                // c: "Allow NFA_MOPEN as a valid postfix representation for the
                // empty regexp" — also covers an empty group.
                if stack.is_empty() {
                    let s = b.alloc_state(mopen, -1, -1);
                    let s1 = b.alloc_state(mclose, -1, -1);
                    b.states[s].out = s1 as i32;
                    stack.push(Frag {
                        start: s,
                        out: list1(s1, 0),
                    });
                } else {
                    let e = stack.pop()?;
                    let s = b.alloc_state(mopen, e.start as i32, -1);
                    let s1 = b.alloc_state(mclose, -1, -1);
                    b.patch(&e.out, s1 as i32);
                    stack.push(Frag {
                        start: s,
                        out: list1(s1, 0),
                    });
                }
            }
            NFA_BACKREF1..=NFA_BACKREF9 => {
                let s = b.alloc_state(op, -1, -1);
                let s1 = b.alloc_state(NFA_SKIP, -1, -1);
                b.states[s].out = s1 as i32;
                stack.push(Frag {
                    start: s,
                    out: list1(s1, 0),
                });
            }
            // c: the `NFA_LNUM … NFA_MARK_LT` arm — an operand follows in the
            // postfix stream and becomes the state's `val`. `NFA_CLASS_OBJ`
            // (this port's own) carries its class index the same way.
            NFA_COL | NFA_COL_GT | NFA_COL_LT | NFA_CLASS_OBJ => {
                p += 1;
                let n = *postfix.get(p)?;
                let s = b.alloc_state(op, -1, -1);
                b.states[s].val = n;
                stack.push(Frag {
                    start: s,
                    out: list1(s, 0),
                });
            }
            // Operands
            _ => {
                let s = b.alloc_state(op, -1, -1);
                stack.push(Frag {
                    start: s,
                    out: list1(s, 0),
                });
            }
        }
        p += 1;
    }

    let e = stack.pop()?;
    if !stack.is_empty() {
        // c: "E876: (NFA regexp) Too many states left on stack"
        return None;
    }
    let matchstate = b.alloc_state(NFA_MATCH, -1, -1);
    b.patch(&e.out, matchstate as i32);
    Some((b.states, e.start))
}

/// c: `nfa_max_width()` — an estimate of the maximum width of anything matching
/// `startstate`, used to bound how far back a `\@<=` looks. `-1` is "unknown or
/// unlimited". Measured in characters here rather than bytes, so the
/// `MB_MAXBYTES` / `has_mbyte ? 3 : 1` weights all collapse to 1.
fn nfa_max_width(states: &[State], startstate: i32, depth: usize) -> i32 {
    let mut state = startstate;
    let mut len = 0i32;
    // detect looping in a NFA_SPLIT
    if depth > 4 {
        return -1;
    }
    while state >= 0 {
        let s = &states[state as usize];
        match s.c {
            NFA_END_INVISIBLE | NFA_END_INVISIBLE_NEG => return len,
            NFA_SPLIT => {
                let l = nfa_max_width(states, s.out, depth + 1);
                let r = nfa_max_width(states, s.out1, depth + 1);
                if l < 0 || r < 0 {
                    return -1;
                }
                return len + if l > r { l } else { r };
            }
            NFA_ANY | NFA_CLASS_OBJ | NFA_ANY_COMPOSING => len += 1,
            NFA_START_INVISIBLE
            | NFA_START_INVISIBLE_NEG
            | NFA_START_INVISIBLE_BEFORE
            | NFA_START_INVISIBLE_BEFORE_NEG => {
                // zero-width, out1 points to the END state
                if s.out1 < 0 {
                    return -1;
                }
                state = states[s.out1 as usize].out;
                continue;
            }
            NFA_BACKREF1..=NFA_BACKREF9 | NFA_SKIP => return -1,
            NFA_BOL | NFA_EOL | NFA_BOF | NFA_EOF | NFA_BOW | NFA_EOW | NFA_NOPEN | NFA_NCLOSE
            | NFA_ZSTART | NFA_ZEND | NFA_OPT_CHARS | NFA_EMPTY | NFA_START_PATTERN
            | NFA_END_PATTERN | NFA_CURSOR | NFA_COL | NFA_COL_GT | NFA_COL_LT => {}
            c if (NFA_MOPEN..=NFA_MOPEN9).contains(&c) => {}
            c if (NFA_MCLOSE..=NFA_MCLOSE9).contains(&c) => {}
            c if c < 0 => return -1,
            // normal character
            _ => len += 1,
        }
        state = s.out;
    }
    -1
}

/// c: `match_follows()` — TRUE if `startstate` leads to `NFA_MATCH` without
/// advancing the input.
fn match_follows(states: &[State], startstate: i32, depth: usize) -> bool {
    let mut state = startstate;
    // avoid too much recursion
    if depth > 10 {
        return false;
    }
    while state >= 0 {
        let s = &states[state as usize];
        match s.c {
            NFA_MATCH
            | NFA_MCLOSE
            | NFA_END_INVISIBLE
            | NFA_END_INVISIBLE_NEG
            | NFA_END_PATTERN => return true,
            NFA_SPLIT => {
                return match_follows(states, s.out, depth + 1)
                    || match_follows(states, s.out1, depth + 1)
            }
            NFA_START_INVISIBLE
            | NFA_START_INVISIBLE_FIRST
            | NFA_START_INVISIBLE_BEFORE
            | NFA_START_INVISIBLE_BEFORE_FIRST
            | NFA_START_INVISIBLE_NEG
            | NFA_START_INVISIBLE_NEG_FIRST
            | NFA_START_INVISIBLE_BEFORE_NEG
            | NFA_START_INVISIBLE_BEFORE_NEG_FIRST => {
                // skip ahead to next state
                if s.out1 < 0 {
                    return false;
                }
                state = states[s.out1 as usize].out;
                continue;
            }
            // states that will advance the input
            NFA_ANY | NFA_ANY_COMPOSING | NFA_CLASS_OBJ => return false,
            c if c > 0 => return false,
            // Others: zero-width or possibly zero-width, might still find a
            // match at the same position, keep looking.
            _ => {}
        }
        state = s.out;
    }
    false
}

/// c: `failure_chance()` — an estimate of how likely matching `state` is to
/// fail, from 0 (always matches) to 99.
fn failure_chance(states: &[State], state: i32, depth: usize) -> i32 {
    if state < 0 {
        return 1;
    }
    let s = &states[state as usize];
    // detect looping
    if depth > 4 {
        return 1;
    }
    match s.c {
        NFA_SPLIT => {
            if s.out >= 0 && states[s.out as usize].c == NFA_SPLIT
                || s.out1 >= 0 && states[s.out1 as usize].c == NFA_SPLIT
            {
                // avoid recursive stuff
                return 1;
            }
            let l = failure_chance(states, s.out, depth + 1);
            let r = failure_chance(states, s.out1, depth + 1);
            if l < r {
                l
            } else {
                r
            }
        }
        // matches anything, unlikely to fail
        NFA_ANY => 1,
        // empty match works always
        NFA_MATCH | NFA_MCLOSE | NFA_ANY_COMPOSING => 0,
        // recursive regmatch is expensive, use low failure chance
        NFA_START_INVISIBLE
        | NFA_START_INVISIBLE_FIRST
        | NFA_START_INVISIBLE_NEG
        | NFA_START_INVISIBLE_NEG_FIRST
        | NFA_START_INVISIBLE_BEFORE
        | NFA_START_INVISIBLE_BEFORE_FIRST
        | NFA_START_INVISIBLE_BEFORE_NEG
        | NFA_START_INVISIBLE_BEFORE_NEG_FIRST
        | NFA_START_PATTERN => 5,
        NFA_BOL | NFA_EOL | NFA_BOF | NFA_EOF => 99,
        NFA_BOW | NFA_EOW => 90,
        NFA_NOPEN => failure_chance(states, s.out, depth + 1),
        // backreferences don't match in many places
        NFA_BACKREF1..=NFA_BACKREF9 => 94,
        // before/after positions don't match very often
        NFA_COL_GT | NFA_COL_LT => 85,
        // specific positions rarely match
        NFA_CURSOR | NFA_COL => 98,
        c if (NFA_MOPEN..=NFA_MOPEN9).contains(&c) => failure_chance(states, s.out, depth + 1),
        c if (NFA_MCLOSE + 1..=NFA_MCLOSE9).contains(&c) => {
            failure_chance(states, s.out, depth + 1)
        }
        c if c == NFA_NCLOSE => failure_chance(states, s.out, depth + 1),
        // character match fails often
        c if c > 0 => 95,
        // something else, includes character classes
        _ => 50,
    }
}

/// c: `nfa_postprocess()` — decide, per invisible-match state, whether to run
/// the sub-match eagerly (the `_FIRST` variants) or postpone it.
fn nfa_postprocess(states: &mut [State]) {
    for i in 0..states.len() {
        let c = states[i].c;
        if c == NFA_START_INVISIBLE
            || c == NFA_START_INVISIBLE_NEG
            || c == NFA_START_INVISIBLE_BEFORE
            || c == NFA_START_INVISIBLE_BEFORE_NEG
        {
            let out1 = states[i].out1;
            if out1 < 0 {
                continue;
            }
            let follows = states[out1 as usize].out;
            let directly;
            // Do it directly when what follows is possibly the end of the match.
            if match_follows(states, follows, 0) {
                directly = true;
            } else {
                let ch_invisible = failure_chance(states, states[i].out, 0);
                let ch_follows = failure_chance(states, follows, 0);
                if c == NFA_START_INVISIBLE_BEFORE || c == NFA_START_INVISIBLE_BEFORE_NEG {
                    // "before" matches are very expensive when unbounded,
                    // always prefer what follows then, unless what follows will
                    // always match.  Otherwise strongly prefer what follows.
                    if states[i].val <= 0 && ch_follows > 0 {
                        directly = false;
                    } else {
                        directly = ch_follows * 10 < ch_invisible;
                    }
                } else {
                    // normal invisible, first do the one with the highest
                    // failure chance
                    directly = ch_follows < ch_invisible;
                }
            }
            if directly {
                // switch to the _FIRST state
                states[i].c += 1;
            }
        }
    }
}

/// c: `nfa_get_reganch()` — "figure out if the NFA state list starts with an
/// anchor, must match at start of the line".
///
/// The C's switch also lists `NFA_VISUAL` and the `NFA_ZOPEN` family; neither
/// is compiled by this port ([`Prog::compile`] returns `None` for `\z(` and for
/// `\%V`), so those arms would be dead code and are not transcribed.
fn nfa_get_reganch(states: &[State], start: i32, depth: usize) -> bool {
    let mut p = start;

    if depth > 4 {
        return false;
    }

    while p >= 0 {
        let st = &states[p as usize];
        match st.c {
            NFA_BOL | NFA_BOF => return true, // yes!

            NFA_ZSTART | NFA_ZEND | NFA_CURSOR | NFA_NOPEN => p = st.out,

            NFA_SPLIT => {
                return nfa_get_reganch(states, st.out, depth + 1)
                    && nfa_get_reganch(states, st.out1, depth + 1)
            }

            c if (NFA_MOPEN..=NFA_MOPEN9).contains(&c) => p = st.out,

            _ => return false, // noooo
        }
    }
    false
}

/// c: `nfa_get_regstart()` — "figure out if the NFA state list starts with a
/// character which must match at start of the match". `0` is the C's `NUL`,
/// meaning "no such character".
///
/// As in [`nfa_get_reganch`], the `NFA_VISUAL` / `NFA_LNUM` / `NFA_VCOL` /
/// `NFA_MARK` / `NFA_ZOPEN` arms of the C's switch are omitted because this
/// port never compiles those opcodes.
fn nfa_get_regstart(states: &[State], start: i32, depth: usize) -> i32 {
    let mut p = start;

    if depth > 4 {
        return 0;
    }

    while p >= 0 {
        let st = &states[p as usize];
        match st.c {
            // all kinds of zero-width matches
            NFA_BOL | NFA_BOF | NFA_BOW | NFA_EOW | NFA_ZSTART | NFA_ZEND | NFA_CURSOR
            | NFA_COL | NFA_COL_GT | NFA_COL_LT | NFA_NOPEN => p = st.out,

            NFA_SPLIT => {
                let c1 = nfa_get_regstart(states, st.out, depth + 1);
                let c2 = nfa_get_regstart(states, st.out1, depth + 1);

                if c1 == c2 {
                    return c1; // yes!
                }
                return 0;
            }

            c if (NFA_MOPEN..=NFA_MOPEN9).contains(&c) => p = st.out,

            c => {
                if c > 0 {
                    return c; // yes!
                }
                return 0;
            }
        }
    }
    0
}

/// c: `nfa_get_match_text()` — "figure out if the NFA state list contains just
/// literal text and nothing else. If so return a string […] with what must
/// match after regstart."
///
/// The C's `len` accumulator only sizes the `alloc()`, so it has no analogue
/// here.
fn nfa_get_match_text(states: &[State], start: usize) -> Option<Vec<char>> {
    if states[start].c != NFA_MOPEN {
        return None; // just in case
    }
    let mut p = states[start].out;
    while p >= 0 && states[p as usize].c > 0 {
        p = states[p as usize].out;
    }
    if p < 0 || states[p as usize].c != NFA_MCLOSE {
        return None;
    }
    let after = states[p as usize].out;
    if after < 0 || states[after as usize].c != NFA_MATCH {
        return None;
    }

    // c: "p = start->out->out;  // skip first char, it goes into regstart".
    // `start->out` is known non-NULL: the walk above dereferenced it.
    let mut p = states[states[start].out as usize].out;
    let mut ret = Vec::new();
    while p >= 0 && states[p as usize].c > 0 {
        ret.push(char::from_u32(states[p as usize].c as u32).unwrap_or('\0'));
        p = states[p as usize].out;
    }
    Some(ret)
}

/// c: `skip_to_start()` — "skip until the char `c` we know a match must start
/// with", returning the new column or `None` for the C's `FAIL`.
///
/// The C picks between `vim_strbyte()` and `cstrchr()`; this port has one
/// subject representation (`&[char]`) and one case-equality rule
/// ([`super::char_eq`]), and it must be *that* rule so the scan can never skip
/// a position the character-matching arm of [`Matcher::regmatch`] would have
/// accepted.
fn skip_to_start(text: &[char], c: i32, col: usize, ic: bool) -> Option<usize> {
    let needle = char::from_u32(c as u32)?;
    text.iter()
        .skip(col)
        .position(|&t| super::char_eq(t, needle, ic))
        .map(|i| col + i)
}

/// c: `find_match_text()` — "check for a match with match_text. Called after
/// `skip_to_start()` has found regstart." Returns the match bounds, or `None`
/// for the C's `0L`; `*startcol` is written through in both cases, as in the C.
///
/// Two of the C's concerns do not survive the change of subject representation
/// and one does not exist in this port:
///
/// * `len2` is a byte length in the C, which is why it needs the `utf_fold()`
///   correction for case-folded text whose folded form is shorter. Here it
///   counts characters, where `regstart` is always exactly one.
/// * `cleanup_subexpr()` and the `reg_startp`/`reg_endp` writes become the
///   returned pair: `match_text` is only ever set for a program that is one
///   literal string, which has no subexpressions past `\0`.
/// * `rex.reg_icombine` (`\Z`) is always false — this port has no `\Z`.
fn find_match_text(
    text: &[char],
    startcol: &mut usize,
    regstart: i32,
    match_text: &[char],
    ic: bool,
) -> Option<(usize, usize)> {
    let mut col = *startcol;

    loop {
        let mut is_match = true;
        // skip regstart
        let mut len2 = 1usize;
        for &c1 in match_text {
            let c2 = text.get(col + len2).copied().unwrap_or('\0');
            if !super::char_eq(c1, c2, ic) {
                is_match = false;
                break;
            }
            len2 += 1;
        }
        if is_match
            // check that no composing char follows
            && !text
                .get(col + len2)
                .is_some_and(|&c| utf_iscomposing(c))
        {
            *startcol = col;
            return Some((col, col + len2));
        }

        // Try finding regstart after the current match.
        col += 1; // skip regstart
        match skip_to_start(text, regstart, col, ic) {
            Some(c) => col = c,
            None => break,
        }
    }

    *startcol = col;
    None
}

impl Prog {
    /// c: `nfa_regcomp()`. `None` means the NFA engine bailed and the caller
    /// must use the backtracker — which is exactly what vim's automatic engine
    /// does when `nfa_regcomp()` returns NULL.
    pub fn compile(branches: &[Branch], ngroups: usize) -> Option<Prog> {
        let mut post = Post {
            post: Vec::new(),
            classes: Vec::new(),
            has_zend: false,
            has_backref: false,
            wants_nfa: false,
        };
        if !post.emit_alt(branches) {
            return None;
        }
        // c: `re2post()` — the whole pattern is wrapped in `\0`.
        post.emit(NFA_MOPEN);
        let (mut states, start) = post2nfa(&post.post)?;
        nfa_postprocess(&mut states);
        // c: the three lines after `nfa_postprocess(prog)` in `nfa_regcomp()`,
        // in that order.
        let reganch = nfa_get_reganch(&states, start as i32, 0);
        let regstart = nfa_get_regstart(&states, start as i32, 0);
        let match_text = nfa_get_match_text(&states, start);
        Some(Prog {
            states,
            start,
            nsubexp: ngroups + 1,
            has_zend: post.has_zend,
            has_backref: post.has_backref,
            classes: post.classes,
            reganch,
            regstart,
            match_text,
        })
    }
}

// ── execution (c: the "NFA execution code" half of regexp_nfa.c) ──

/// A recorded subexpression position, or the C's NULL pointer.
///
/// Stored as a bare `u32` with `u32::MAX` for NULL rather than as an
/// `Option<usize>`: a [`Thread`] carries twenty of these plus a [`Pim`]'s
/// twenty more, and every `addstate()` writes a whole thread. The enum layout
/// made one thread 696 bytes; this makes it 216. A subject longer than 4Gi
/// characters cannot be addressed, which no VimL string is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Pos(u32);

/// c: a NULL `list.line[i].start` / `.end` — the subexpression is not set.
const NO_POS: Pos = Pos(u32::MAX);

impl Pos {
    fn at(v: usize) -> Pos {
        Pos(u32::try_from(v).unwrap_or(u32::MAX - 1))
    }
    fn get(self) -> Option<usize> {
        (self != NO_POS).then_some(self.0 as usize)
    }
    fn is_set(self) -> bool {
        self != NO_POS
    }
}

/// c: `regsub_T` with `REG_MULTI == FALSE`, i.e. the `list.line` arm.
#[derive(Clone, PartialEq, Eq, Debug)]
struct RegSub {
    in_use: usize,
    list: [(Pos, Pos); NSUBEXP],
}

impl Default for RegSub {
    fn default() -> Self {
        RegSub {
            in_use: 0,
            list: [(NO_POS, NO_POS); NSUBEXP],
        }
    }
}

impl RegSub {
    /// c: `copy_sub()`.
    fn copy_from(&mut self, from: &RegSub) {
        self.in_use = from.in_use;
        if from.in_use == 0 {
            return;
        }
        self.list[..from.in_use].copy_from_slice(&from.list[..from.in_use]);
    }

    /// c: `copy_sub_off()` — like `copy_sub()` but excluding the main match.
    fn copy_off_from(&mut self, from: &RegSub) {
        if self.in_use < from.in_use {
            self.in_use = from.in_use;
        }
        if from.in_use <= 1 {
            return;
        }
        self.list[1..from.in_use].copy_from_slice(&from.list[1..from.in_use]);
    }

    /// c: `sub_equal()`. With backreferences the end positions matter too.
    fn equal(&self, other: &RegSub, has_backref: bool) -> bool {
        let todo = self.in_use.max(other.in_use);
        for i in 0..todo {
            let sp1 = if i < self.in_use {
                self.list[i].0
            } else {
                NO_POS
            };
            let sp2 = if i < other.in_use {
                other.list[i].0
            } else {
                NO_POS
            };
            if sp1 != sp2 {
                return false;
            }
            if has_backref {
                let ep1 = if i < self.in_use {
                    self.list[i].1
                } else {
                    NO_POS
                };
                let ep2 = if i < other.in_use {
                    other.list[i].1
                } else {
                    NO_POS
                };
                if ep1 != ep2 {
                    return false;
                }
            }
        }
        true
    }
}

/// c: the `NFA_PIM_*` values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PimResult {
    Unused,
    Todo,
    Match,
    NoMatch,
}

/// c: `nfa_pim_T` — a Postponed Invisible Match.
#[derive(Clone, Debug)]
struct Pim {
    result: PimResult,
    state: usize,
    subs: RegSub,
    end: usize,
}

impl Default for Pim {
    fn default() -> Self {
        Pim {
            result: PimResult::Unused,
            state: 0,
            subs: RegSub::default(),
            end: 0,
        }
    }
}

impl Pim {
    /// c: `pim_equal()` — including when both are unused.
    fn equal(one: Option<&Pim>, two: Option<&Pim>) -> bool {
        let one_unused = !matches!(one, Some(p) if p.result != PimResult::Unused);
        let two_unused = !matches!(two, Some(p) if p.result != PimResult::Unused);
        if one_unused {
            return two_unused;
        }
        if two_unused {
            return false;
        }
        let (o, t) = (one.unwrap(), two.unwrap());
        o.state == t.state && o.end == t.end
    }
}

/// c: `nfa_thread_T`.
#[derive(Clone, Debug)]
struct Thread {
    state: usize,
    count: i32,
    pim: Pim,
    subs: RegSub,
}

/// c: `nfa_list_T`.
struct ThreadList {
    t: Vec<Thread>,
    /// c: `l->len` — tracked separately from `t.len()` because the
    /// `'maxmempattern'` check keys off the *capacity* growth, not the count.
    len: usize,
    id: u32,
    has_pim: bool,
}

/// The submatch positions a successful match produced, in char indices.
pub struct NfaMatch {
    pub subs: Vec<Option<(usize, usize)>>,
}

struct Matcher<'a> {
    prog: &'a Prog,
    text: &'a [char],
    ic: bool,
    /// c: `state->lastlist[2]`, kept out of the program so a compiled `Prog`
    /// stays shareable and immutable across matches.
    lastlist: Vec<[u32; 2]>,
    nfa_ll_index: usize,
    nfa_listid: u32,
    nfa_alt_listid: u32,
    /// c: `rex.input - rex.line`, a char index into `text`.
    input: usize,
    /// c: `nfa_endp` — a required end-of-match position, set for `\@<=`.
    nfa_endp: Option<usize>,
    nfa_match: MatchResult,
    depth: usize,
    /// Set when the run raised `E363`; the caller reports it once.
    err: Option<&'static str>,
}

/// c: `nfa_re_num_cmp()`.
fn nfa_re_num_cmp(val: u32, op: i32, pos: u32) -> bool {
    match op {
        1 => pos > val,
        2 => pos < val,
        _ => val == pos,
    }
}

impl<'a> Matcher<'a> {
    fn new(prog: &'a Prog, text: &'a [char], ic: bool) -> Self {
        Matcher {
            prog,
            text,
            ic,
            lastlist: vec![[0, 0]; prog.states.len()],
            nfa_ll_index: 0,
            // c: `rex.nfa_listid = 1; rex.nfa_alt_listid = 2;`
            nfa_listid: 1,
            nfa_alt_listid: 2,
            input: 0,
            nfa_endp: None,
            nfa_match: MatchResult::No,
            depth: 0,
            err: None,
        }
    }

    /// c: `curc` — `mb_ptr2char(rex.input)`, or NUL at the end of the line.
    fn curc(&self) -> char {
        self.text.get(self.input).copied().unwrap_or('\0')
    }

    /// c: `clen` — `mb_ptr2len(rex.input)`, which for UTF-8 is `utfc_ptr2len`:
    /// the base character AND its composing marks. 0 at end of input.
    fn clen(&self) -> usize {
        if self.input >= self.text.len() {
            return 0;
        }
        let mut end = self.input + 1;
        while self.text.get(end).is_some_and(|c| utf_iscomposing(*c)) {
            end += 1;
        }
        end - self.input
    }

    /// c: `has_state_with_pos()`.
    fn has_state_with_pos(
        &self,
        l: &ThreadList,
        state: usize,
        subs: &RegSub,
        pim: Option<&Pim>,
    ) -> bool {
        l.t.iter().any(|th| {
            th.state == state
                && th.subs.equal(subs, self.prog.has_backref)
                && Pim::equal(Some(&th.pim), pim)
        })
    }

    /// c: `state_in_list()`.
    fn state_in_list(&self, l: &ThreadList, state: usize, subs: &RegSub) -> bool {
        self.lastlist[state][self.nfa_ll_index] == l.id
            && (!self.prog.has_backref || self.has_state_with_pos(l, state, subs, None))
    }

    /// c: `addstate()`. Returns `false` when the C returns NULL — recursion too
    /// deep, or `'maxmempattern'` exceeded.
    ///
    /// The `subs_arg` / `temp_subs` dance in the C exists only because `subs`
    /// may point INTO the array being reallocated; Rust's `RegSub` is owned, so
    /// the parameter is passed by `&mut` and the C's "subs may have changed,
    /// need to set sub again" reload is a no-op here.
    fn addstate(
        &mut self,
        l: &mut ThreadList,
        state: i32,
        subs: &mut RegSub,
        pim: Option<&Pim>,
        off_arg: i32,
    ) -> bool {
        if state < 0 {
            return true;
        }
        let state = state as usize;
        // This function is called recursively.  When the depth is too much we
        // run out of stack and crash, limit recursiveness here.
        self.depth += 1;
        if self.depth >= MAX_ADDSTATE_DEPTH {
            self.depth -= 1;
            return false;
        }
        let ok = self.addstate_inner(l, state, subs, pim, off_arg);
        self.depth -= 1;
        ok
    }

    fn addstate_inner(
        &mut self,
        l: &mut ThreadList,
        state: usize,
        subs: &mut RegSub,
        pim: Option<&Pim>,
        off_arg: i32,
    ) -> bool {
        let mut add_here = false;
        let mut off = off_arg;
        let mut listindex = 0usize;
        if off_arg <= -ADDSTATE_HERE_OFFSET {
            add_here = true;
            off = 0;
            listindex = (-(off_arg + ADDSTATE_HERE_OFFSET)) as usize;
        }

        let c = self.prog.states[state].c;
        let skip_registration =
            matches!(c, NFA_NCLOSE | NFA_MOPEN | NFA_ZEND | NFA_SPLIT | NFA_EMPTY)
                || (NFA_MCLOSE..=NFA_MCLOSE9).contains(&c);
        if !skip_registration {
            // c: the NFA_BOL/NFA_BOF pre-test — "^" won't match past
            // end-of-line, don't bother trying.
            if (c == NFA_BOL || c == NFA_BOF) && self.input > 0 && self.input < self.text.len() {
                return true;
            }
            if self.lastlist[state][self.nfa_ll_index] == l.id && c != NFA_SKIP {
                // This state is already in the list, don't add it again, unless
                // it is an MOPEN that is used for a backreference or when there
                // is a PIM. For NFA_MATCH check the position, lower position is
                // preferred.
                if !self.prog.has_backref && pim.is_none() && !l.has_pim && c != NFA_MATCH {
                    let mut found = false;
                    if add_here {
                        for k in 0..l.t.len().min(listindex) {
                            if l.t[k].state == state {
                                found = true;
                                break;
                            }
                        }
                    }
                    if !add_here || found {
                        return true;
                    }
                }
                // Do not add the state again when it exists with the same
                // positions.
                if self.has_state_with_pos(l, state, subs, pim) {
                    return true;
                }
            }

            // When there are backreferences or PIMs the number of states may be
            // (a lot) bigger than anticipated.
            if l.t.len() == l.len {
                let newlen = l.len * 3 / 2 + 50;
                let newsize = newlen * SIZEOF_NFA_THREAD_T;
                if (newsize >> 10) >= P_MMP {
                    self.err = Some(E_MAXMEMPATTERN);
                    return false;
                }
                l.len = newlen;
            }

            // add the state to the list
            self.lastlist[state][self.nfa_ll_index] = l.id;
            let mut th = Thread {
                state,
                count: 0,
                pim: Pim::default(),
                subs: RegSub::default(),
            };
            match pim {
                None => th.pim.result = PimResult::Unused,
                Some(p) => {
                    th.pim = p.clone();
                    l.has_pim = true;
                }
            }
            th.subs.copy_from(subs);
            l.t.push(th);
        }

        match c {
            NFA_MATCH => true,
            NFA_SPLIT => {
                // order matters here
                let (out, out1) = (self.prog.states[state].out, self.prog.states[state].out1);
                if !self.addstate(l, out, subs, pim, off_arg) {
                    return false;
                }
                self.addstate(l, out1, subs, pim, off_arg)
            }
            NFA_EMPTY | NFA_NOPEN | NFA_NCLOSE => {
                let out = self.prog.states[state].out;
                self.addstate(l, out, subs, pim, off_arg)
            }
            NFA_ZSTART | NFA_MOPEN..=NFA_MOPEN9 => {
                let subidx = if c == NFA_ZSTART {
                    0
                } else {
                    (c - NFA_MOPEN) as usize
                };
                if subidx >= NSUBEXP {
                    return true;
                }
                // Set the position (with "off" added) in the subexpression.
                // Save and restore it when it was in use. Otherwise fill any gap.
                let save_ptr;
                let save_in_use;
                if subidx < subs.in_use {
                    save_ptr = subs.list[subidx].0;
                    save_in_use = usize::MAX; // c: -1
                } else {
                    save_in_use = subs.in_use;
                    for i in subs.in_use..subidx {
                        subs.list[i] = (NO_POS, NO_POS);
                    }
                    subs.in_use = subidx + 1;
                    save_ptr = NO_POS;
                }
                subs.list[subidx].0 = Pos::at((self.input as i32 + off) as usize);

                let out = self.prog.states[state].out;
                let ok = self.addstate(l, out, subs, pim, off_arg);
                if !ok {
                    return false;
                }
                if save_in_use == usize::MAX {
                    subs.list[subidx].0 = save_ptr;
                } else {
                    subs.in_use = save_in_use;
                }
                true
            }
            NFA_ZEND | NFA_MCLOSE..=NFA_MCLOSE9 => {
                // c: the NFA_MCLOSE arm skips the write when \ze already set an
                // end, so \ze's position is not overwritten.
                if c == NFA_MCLOSE && self.prog.has_zend && subs.list[0].1.is_set() {
                    let out = self.prog.states[state].out;
                    return self.addstate(l, out, subs, pim, off_arg);
                }
                let subidx = if c == NFA_ZEND {
                    0
                } else {
                    (c - NFA_MCLOSE) as usize
                };
                if subidx >= NSUBEXP {
                    return true;
                }
                // We don't fill in gaps here, there must have been an MOPEN
                // that has done that.
                let save_in_use = subs.in_use;
                if subs.in_use <= subidx {
                    subs.in_use = subidx + 1;
                }
                let save_ptr = subs.list[subidx].1;
                subs.list[subidx].1 = Pos::at((self.input as i32 + off) as usize);

                let out = self.prog.states[state].out;
                let ok = self.addstate(l, out, subs, pim, off_arg);
                if !ok {
                    return false;
                }
                subs.list[subidx].1 = save_ptr;
                subs.in_use = save_in_use;
                true
            }
            _ => true,
        }
    }

    /// c: `addstate_here()` — put the new state(s) at position `*ip` so the
    /// order of states to be tried does not change, which matters for
    /// alternatives.
    fn addstate_here(
        &mut self,
        l: &mut ThreadList,
        state: i32,
        subs: &mut RegSub,
        pim: Option<&Pim>,
        ip: &mut usize,
    ) -> bool {
        let tlen = l.t.len();
        let listidx = *ip;
        if !self.addstate(
            l,
            state,
            subs,
            pim,
            -(listidx as i32) - ADDSTATE_HERE_OFFSET,
        ) {
            return false;
        }
        // when "*ip" was at the end of the list, nothing to do
        if listidx + 1 == tlen {
            return true;
        }
        let count = l.t.len() - tlen;
        if count == 0 {
            return true; // no state got added
        }
        if count == 1 {
            // overwrite the current state
            let last = l.t.len() - 1;
            l.t[listidx] = l.t[last].clone();
        } else {
            // make space for new states, then move them from the end to the
            // current position
            let moved: Vec<Thread> = l.t[l.t.len() - count..].to_vec();
            l.t.truncate(l.t.len() - count);
            l.t.splice(listidx..listidx + 1, moved);
            return {
                *ip = listidx.wrapping_sub(1);
                true
            };
        }
        l.t.pop();
        *ip = listidx.wrapping_sub(1);
        true
    }

    /// c: `match_backref()` — check for a match with subexpression `subidx`.
    /// Returns `(matched, len)` with `len` in characters.
    fn match_backref(&self, sub: &RegSub, subidx: usize) -> (bool, usize) {
        if sub.in_use <= subidx {
            // backref was not set, match an empty string
            return (true, 0);
        }
        let (Some(s), Some(e)) = (sub.list[subidx].0.get(), sub.list[subidx].1.get()) else {
            return (true, 0);
        };
        if e < s || e > self.text.len() {
            return (true, 0);
        }
        let len = e - s;
        if self.input + len > self.text.len() {
            return (false, 0);
        }
        for i in 0..len {
            if !super::char_eq(self.text[s + i], self.text[self.input + i], self.ic) {
                return (false, 0);
            }
        }
        (true, len)
    }

    /// c: `recursive_regmatch()`.
    fn recursive_regmatch(
        &mut self,
        state: usize,
        pim: Option<&Pim>,
        submatch: &mut RegSub,
        m: &mut RegSub,
    ) -> MatchResult {
        let save_reginput = self.input;
        let save_nfa_match = self.nfa_match;
        let save_nfa_listid = self.nfa_listid;
        let save_nfa_endp = self.nfa_endp;
        let mut endposp = None;
        let mut need_restore = false;

        if let Some(p) = pim {
            // start at the position where the postponed match was
            self.input = p.end;
        }

        let c = self.prog.states[state].c;
        if c == NFA_START_INVISIBLE_BEFORE
            || c == NFA_START_INVISIBLE_BEFORE_FIRST
            || c == NFA_START_INVISIBLE_BEFORE_NEG
            || c == NFA_START_INVISIBLE_BEFORE_NEG_FIRST
        {
            // The recursive match must end at the current position.
            endposp = Some(match pim {
                None => self.input,
                Some(p) => p.end,
            });
            // Go back the specified number of characters to try matching
            // "\@<=" or not matching "\@<!".
            let val = self.prog.states[state].val;
            if val <= 0 {
                self.input = 0;
            } else if self.input >= val as usize {
                self.input -= val as usize;
            } else {
                self.input = 0;
            }
        }

        // Have to clear the lastlist field of the NFA nodes, so that
        // nfa_regmatch() and addstate() can run properly after recursion.
        let mut saved_listids: Vec<u32> = Vec::new();
        if self.nfa_ll_index == 1 {
            // Already calling nfa_regmatch() recursively.  Save the lastlist[1]
            // values and clear them.
            saved_listids = self.lastlist.iter().map(|l| l[1]).collect();
            for l in self.lastlist.iter_mut() {
                l[1] = 0;
            }
            need_restore = true;
        } else {
            // First recursive nfa_regmatch() call, switch to the second
            // lastlist entry.
            self.nfa_ll_index += 1;
            if self.nfa_listid <= self.nfa_alt_listid {
                self.nfa_listid = self.nfa_alt_listid;
            }
        }

        self.nfa_endp = endposp;
        let out = self.prog.states[state].out;
        let result = self.regmatch(out, submatch, m);

        if need_restore {
            for (l, v) in self.lastlist.iter_mut().zip(saved_listids) {
                l[1] = v;
            }
        } else {
            self.nfa_ll_index -= 1;
            self.nfa_alt_listid = self.nfa_listid;
        }

        // restore position in input text
        self.input = save_reginput;
        if result != MatchResult::TooExpensive {
            self.nfa_match = save_nfa_match;
            self.nfa_listid = save_nfa_listid;
        }
        self.nfa_endp = save_nfa_endp;
        result
    }

    /// c: `nfa_regmatch()` — run the NFA to determine whether it matches at
    /// `rex.input`.
    fn regmatch(&mut self, start: i32, submatch: &mut RegSub, m: &mut RegSub) -> MatchResult {
        if start < 0 {
            return MatchResult::No;
        }
        let start = start as usize;
        let toplevel = self.prog.states[start].c == NFA_MOPEN;
        self.nfa_match = MatchResult::No;

        let cap = self.prog.states.len() + 1;
        let mut list = [
            ThreadList {
                t: Vec::new(),
                len: cap,
                id: 0,
                has_pim: false,
            },
            ThreadList {
                t: Vec::new(),
                len: cap,
                id: 0,
                has_pim: false,
            },
        ];
        let mut flag = 0usize;
        list[0].id = self.nfa_listid + 1;

        // Inline optimized code for addstate(thislist, start, m, 0) if we know
        // it's the first MOPEN.
        {
            let (this, _) = split_lists(&mut list, 0);
            let ok = if toplevel {
                m.list[0].0 = Pos::at(self.input);
                m.in_use = 1;
                let out = self.prog.states[start].out;
                self.addstate(this, out, m, None, 0)
            } else {
                self.addstate(this, start as i32, m, None, 0)
            };
            if !ok {
                return MatchResult::TooExpensive;
            }
        }

        let mut go_to_nextline;
        loop {
            let curc = self.curc();
            let mut clen = self.clen();
            go_to_nextline = false;

            // swap lists
            let thisidx = flag;
            flag ^= 1;
            let nextidx = flag;
            list[nextidx].t.clear();
            list[nextidx].has_pim = false;
            self.nfa_listid += 1;
            if self.nfa_listid >= NFA_MAX_STATES {
                // too many states, retry with old engine
                return MatchResult::TooExpensive;
            }
            list[thisidx].id = self.nfa_listid;
            list[nextidx].id = self.nfa_listid + 1;

            // If the state lists are empty we can stop.
            if list[thisidx].t.is_empty() {
                break;
            }

            let mut listidx = 0usize;
            let mut goto_nextchar = false;
            while listidx < list[thisidx].t.len() {
                // The C keeps a POINTER into the list here (`t = &thislist->t[listidx]`).
                // Rust cannot, because the arms below hand the list to
                // `addstate()`, so the fields are read out instead — the
                // scalars always, and the ~340 bytes of submatch state only in
                // the arms that actually look at it.
                let t_state = list[thisidx].t[listidx].state;
                let t_count = list[thisidx].t[listidx].count;
                let t_pim_unused = list[thisidx].t[listidx].pim.result == PimResult::Unused;
                let mut add_state: i32 = -1;
                let mut add_here = false;
                let mut add_count = 0i32;
                let mut add_off = 0i32;
                let result;
                let st = self.prog.states[t_state].clone();

                match st.c {
                    NFA_MATCH => {
                        // If the match ends before a composing character and
                        // 'reg_icombine' is not set, that is not really a match.
                        if self.input != 0 && utf_iscomposing(curc) {
                            listidx += 1;
                            continue;
                        }
                        self.nfa_match = MatchResult::Yes;
                        submatch.copy_from(&list[thisidx].t[listidx].subs);
                        // Found the left-most longest match, do not look at any
                        // other states at this position.
                        if list[nextidx].t.is_empty() {
                            clen = 0;
                        }
                        goto_nextchar = true;
                        break;
                    }
                    NFA_END_INVISIBLE | NFA_END_INVISIBLE_NEG | NFA_END_PATTERN => {
                        // If "nfa_endp" is set it's only a match if it ends at
                        // "nfa_endp".
                        if self.nfa_endp.is_some_and(|e| self.input != e) {
                            listidx += 1;
                            continue;
                        }
                        // do not set submatches for \@!
                        if st.c != NFA_END_INVISIBLE_NEG {
                            m.copy_from(&list[thisidx].t[listidx].subs);
                        }
                        self.nfa_match = MatchResult::Yes;
                        if list[nextidx].t.is_empty() {
                            clen = 0;
                        }
                        goto_nextchar = true;
                        break;
                    }
                    NFA_START_INVISIBLE
                    | NFA_START_INVISIBLE_FIRST
                    | NFA_START_INVISIBLE_NEG
                    | NFA_START_INVISIBLE_NEG_FIRST
                    | NFA_START_INVISIBLE_BEFORE
                    | NFA_START_INVISIBLE_BEFORE_FIRST
                    | NFA_START_INVISIBLE_BEFORE_NEG
                    | NFA_START_INVISIBLE_BEFORE_NEG_FIRST => {
                        // Do it directly if there already is a PIM or when
                        // nfa_postprocess() detected it will work better.
                        if !t_pim_unused
                            || st.c == NFA_START_INVISIBLE_FIRST
                            || st.c == NFA_START_INVISIBLE_NEG_FIRST
                            || st.c == NFA_START_INVISIBLE_BEFORE_FIRST
                            || st.c == NFA_START_INVISIBLE_BEFORE_NEG_FIRST
                        {
                            let in_use = m.in_use;
                            // Copy submatch info for the recursive call,
                            // opposite of what happens on success below.
                            m.copy_off_from(&list[thisidx].t[listidx].subs);

                            // First try matching the invisible match, then what
                            // follows.
                            let r = self.recursive_regmatch(t_state, None, submatch, m);
                            if r == MatchResult::TooExpensive {
                                return r;
                            }
                            let neg = is_neg_invisible(st.c);
                            if (r == MatchResult::Yes) != neg {
                                let mut subs = list[thisidx].t[listidx].subs.clone();
                                subs.copy_off_from(m);
                                // If the pattern has \ze and it matched in the
                                // sub pattern, use it.
                                if self.prog.has_zend && m.list[0].1.is_set() {
                                    subs.list[0].1 = m.list[0].1;
                                }
                                list[thisidx].t[listidx].subs = subs;
                                // t->state->out1 is the corresponding
                                // END_INVISIBLE node; add its out to the
                                // current list (zero-width match).
                                add_here = true;
                                add_state = out_of_out1(&self.prog.states, &st);
                            }
                            m.in_use = in_use;
                        } else {
                            // First try matching what follows.  Only if a match
                            // is found verify the invisible match matches.
                            let pim = Pim {
                                state: t_state,
                                result: PimResult::Todo,
                                subs: RegSub::default(),
                                end: self.input,
                            };
                            let target = out_of_out1(&self.prog.states, &st);
                            let mut subs = list[thisidx].t[listidx].subs.clone();
                            let (this, _) = split_lists(&mut list, thisidx);
                            if !self.addstate_here(
                                this,
                                target,
                                &mut subs,
                                Some(&pim),
                                &mut listidx,
                            ) {
                                return self.expensive();
                            }
                            listidx = listidx.wrapping_add(1);
                            continue;
                        }
                    }
                    NFA_START_PATTERN => {
                        // There is no point in trying to match the pattern if
                        // the output state is not going to be added to the list.
                        let o1 = out_of_out1(&self.prog.states, &st);
                        let o2 = if o1 >= 0 {
                            self.prog.states[o1 as usize].out
                        } else {
                            -1
                        };
                        let skip = (o1 >= 0
                            && self.state_in_list(
                                &list[nextidx],
                                o1 as usize,
                                &list[thisidx].t[listidx].subs,
                            ))
                            || (o2 >= 0
                                && (self.state_in_list(
                                    &list[nextidx],
                                    o2 as usize,
                                    &list[thisidx].t[listidx].subs,
                                ) || self.state_in_list(
                                    &list[thisidx],
                                    o2 as usize,
                                    &list[thisidx].t[listidx].subs,
                                )));
                        if skip {
                            listidx += 1;
                            continue;
                        }
                        m.copy_off_from(&list[thisidx].t[listidx].subs);
                        let r = self.recursive_regmatch(t_state, None, submatch, m);
                        if r == MatchResult::TooExpensive {
                            return r;
                        }
                        if r == MatchResult::Yes {
                            let mut subs = list[thisidx].t[listidx].subs.clone();
                            subs.copy_off_from(m);
                            list[thisidx].t[listidx].subs = subs;
                            let bytelen =
                                m.list[0].1.get().unwrap_or(self.input) as i32 - self.input as i32;
                            if bytelen == 0 {
                                add_here = true;
                                add_state = o2;
                            } else if bytelen <= clen as i32 {
                                add_state = o2;
                                add_off = clen as i32;
                            } else {
                                add_state = o1;
                                add_off = bytelen;
                                add_count = bytelen - clen as i32;
                            }
                        }
                    }
                    NFA_BOL => {
                        if self.input == 0 {
                            add_here = true;
                            add_state = st.out;
                        }
                    }
                    NFA_EOL => {
                        if curc == '\0' {
                            add_here = true;
                            add_state = st.out;
                        }
                    }
                    NFA_BOW => {
                        // c: `result` starts TRUE and is cleared by each of the
                        // `has_mbyte` arm's tests; `bow_matches` is that arm,
                        // shared with the backtracking engine's `case BOW:`
                        // because regexp_nfa.c:6321 and regexp_bt.c:3517 are
                        // the same three lines.
                        result = super::bow_matches(self.text, self.input);
                        if result {
                            add_here = true;
                            add_state = st.out;
                        }
                    }
                    NFA_EOW => {
                        result = super::eow_matches(self.text, self.input);
                        if result {
                            add_here = true;
                            add_state = st.out;
                        }
                    }
                    NFA_BOF => {
                        if self.input == 0 {
                            add_here = true;
                            add_state = st.out;
                        }
                    }
                    NFA_EOF => {
                        if curc == '\0' {
                            add_here = true;
                            add_state = st.out;
                        }
                    }
                    NFA_ANY => {
                        // Any char except '\0', (end of input) does not match.
                        if curc != '\0' {
                            add_state = st.out;
                            add_off = clen as i32;
                        }
                    }
                    NFA_ANY_COMPOSING => {
                        // On a composing character skip over it.  Otherwise do
                        // nothing.  Always matches.
                        if utf_iscomposing(curc) {
                            add_off = clen as i32;
                        } else {
                            add_here = true;
                            add_off = 0;
                        }
                        add_state = st.out;
                    }
                    NFA_CLASS_OBJ => {
                        let cl = &self.prog.classes[st.val as usize];
                        result = curc != '\0' && cl.matches(curc, self.ic);
                        if result {
                            add_state = st.out;
                            add_off = clen as i32;
                        }
                    }
                    NFA_BACKREF1..=NFA_BACKREF9 => {
                        let subidx = (st.c - NFA_BACKREF1 + 1) as usize;
                        let (ok, bytelen) =
                            self.match_backref(&list[thisidx].t[listidx].subs.clone(), subidx);
                        if ok {
                            if bytelen == 0 {
                                // empty match always works, output of NFA_SKIP
                                // to be used next
                                add_here = true;
                                add_state = if st.out >= 0 {
                                    self.prog.states[st.out as usize].out
                                } else {
                                    -1
                                };
                            } else if bytelen <= clen {
                                add_state = if st.out >= 0 {
                                    self.prog.states[st.out as usize].out
                                } else {
                                    -1
                                };
                                add_off = clen as i32;
                            } else {
                                add_state = st.out;
                                add_off = bytelen as i32;
                                add_count = bytelen as i32 - clen as i32;
                            }
                        }
                    }
                    NFA_SKIP => {
                        // character of previous matching \1 .. \9 or \@>
                        if t_count - (clen as i32) <= 0 {
                            // end of match, go to what follows
                            add_state = st.out;
                            add_off = clen as i32;
                        } else {
                            // add state again with decremented count
                            add_state = t_state as i32;
                            add_off = 0;
                            add_count = t_count - clen as i32;
                        }
                    }
                    NFA_COL | NFA_COL_GT | NFA_COL_LT => {
                        // c: `\%23c` is a BYTE column, 1-based.
                        let col = self.text[..self.input]
                            .iter()
                            .map(|c| c.len_utf8())
                            .sum::<usize>() as u32
                            + 1;
                        result = nfa_re_num_cmp(st.val as u32, st.c - NFA_COL, col);
                        if result {
                            add_here = true;
                            add_state = st.out;
                        }
                    }
                    // c: NFA_CURSOR — `rex.reg_win != NULL && …`. A string match
                    // has no window, so this never matches.
                    NFA_CURSOR => {}
                    // c: these states are only added to be able to bail out when
                    // they are added again, nothing is to be done.
                    NFA_NOPEN | NFA_ZSTART => {}
                    c if (NFA_MOPEN + 1..=NFA_MOPEN9).contains(&c) => {}
                    // regular character
                    c if c > 0 => {
                        result =
                            super::char_eq(char::from_u32(c as u32).unwrap_or('\0'), curc, self.ic);
                        // A literal consumes only the base character, so a
                        // composing mark after it is left for the next state —
                        // and NFA_MATCH refuses to end in front of one.
                        if result {
                            clen = 1;
                            add_state = st.out;
                            add_off = clen as i32;
                        }
                    }
                    _ => {}
                }

                if add_state >= 0 {
                    let mut pim_owned: Option<Pim> = if t_pim_unused {
                        None
                    } else {
                        Some(list[thisidx].t[listidx].pim.clone())
                    };
                    let mut subs = list[thisidx].t[listidx].subs.clone();

                    // Handle the postponed invisible match if the match might
                    // end without advancing and before the end of the line.
                    if pim_owned.is_some()
                        && (clen == 0 || match_follows(&self.prog.states, add_state, 0))
                    {
                        let mut pim = pim_owned.take().unwrap();
                        let neg = is_neg_invisible(self.prog.states[pim.state].c);
                        if pim.result == PimResult::Todo {
                            let pim_for_call = pim.clone();
                            let r = self.recursive_regmatch(
                                pim.state,
                                Some(&pim_for_call),
                                submatch,
                                m,
                            );
                            pim.result = if r == MatchResult::Yes {
                                PimResult::Match
                            } else {
                                PimResult::NoMatch
                            };
                            if (r == MatchResult::Yes) != neg {
                                pim.subs.copy_off_from(m);
                            }
                        }
                        let matched = pim.result == PimResult::Match;
                        if matched != neg {
                            subs.copy_off_from(&pim.subs);
                            list[thisidx].t[listidx].subs = subs.clone();
                        } else {
                            // look-behind match failed, don't add the state
                            listidx += 1;
                            continue;
                        }
                    }

                    let ok = if add_here {
                        let (this, _) = split_lists(&mut list, thisidx);
                        self.addstate_here(
                            this,
                            add_state,
                            &mut subs,
                            pim_owned.as_ref(),
                            &mut listidx,
                        )
                    } else {
                        let (next, _) = split_lists(&mut list, nextidx);
                        let ok =
                            self.addstate(next, add_state, &mut subs, pim_owned.as_ref(), add_off);
                        if ok && add_count > 0 {
                            if let Some(last) = next.t.last_mut() {
                                last.count = add_count;
                            }
                        }
                        ok
                    };
                    if !ok {
                        return self.expensive();
                    }
                }
                listidx = listidx.wrapping_add(1);
            }

            if !goto_nextchar {
                // Look for the start of a match in the current position by
                // adding the start state to the list of states. The first found
                // match is the leftmost one, thus the order of states matters!
                if self.nfa_match == MatchResult::No
                    && ((toplevel && clen != 0) || self.nfa_endp.is_some_and(|e| self.input < e))
                {
                    if toplevel {
                        // Inline optimized code for addstate() if we know the
                        // state is the first MOPEN.
                        let mut add = true;
                        if self.prog.regstart != 0 && clen != 0 {
                            if list[nextidx].t.is_empty() {
                                // Nextlist is empty, we can skip ahead to the
                                // character that must appear at the start.
                                let col = self.input + clen;
                                match skip_to_start(self.text, self.prog.regstart, col, self.ic) {
                                    Some(c) => self.input = c - clen,
                                    None => break,
                                }
                            } else {
                                // Checking if the required start character
                                // matches is cheaper than adding a state that
                                // won't match.
                                let c = self.text.get(self.input + clen).copied().unwrap_or('\0');
                                let rs = char::from_u32(self.prog.regstart as u32).unwrap_or('\0');
                                if !super::char_eq(c, rs, self.ic) {
                                    add = false;
                                }
                            }
                        }

                        if add {
                            m.list[0].0 = Pos::at(self.input + clen);
                            let out = self.prog.states[start].out;
                            let (next, _) = split_lists(&mut list, nextidx);
                            if !self.addstate(next, out, m, None, clen as i32) {
                                return self.expensive();
                            }
                        }
                    } else {
                        let (next, _) = split_lists(&mut list, nextidx);
                        if !self.addstate(next, start as i32, m, None, clen as i32) {
                            return self.expensive();
                        }
                    }
                }
            }

            // Advance to the next character, or finish. The C's third arm
            // (`reg_nextline()`) is unreachable here: `go_to_nextline` is only
            // set by `NFA_NEWL`, which a single-string match never compiles.
            if clen == 0 || go_to_nextline {
                break;
            }
            self.input += clen;
        }

        self.nfa_match
    }

    fn expensive(&self) -> MatchResult {
        MatchResult::TooExpensive
    }
}

/// c: the `\@!` / `\@<!` family test, written out four times in `nfa_regmatch()`.
fn is_neg_invisible(c: i32) -> bool {
    c == NFA_START_INVISIBLE_NEG
        || c == NFA_START_INVISIBLE_NEG_FIRST
        || c == NFA_START_INVISIBLE_BEFORE_NEG
        || c == NFA_START_INVISIBLE_BEFORE_NEG_FIRST
}

/// c: `t->state->out1->out` — the state after the matching `NFA_END_INVISIBLE`
/// / `NFA_END_PATTERN`.
fn out_of_out1(states: &[State], st: &State) -> i32 {
    if st.out1 < 0 {
        -1
    } else {
        states[st.out1 as usize].out
    }
}

/// Borrow one of the two thread lists mutably. The C indexes `list[2]`
/// directly; Rust needs the split to be explicit.
fn split_lists(list: &mut [ThreadList; 2], idx: usize) -> (&mut ThreadList, ()) {
    (&mut list[idx], ())
}

/// What one run of the engine produced. c: `nfa_regexec_both()`'s return value
/// (`> 0` / `0` / `NFA_TOO_EXPENSIVE`) plus the message it may have raised.
pub enum Outcome {
    Match(NfaMatch),
    NoMatch,
    /// c: `NFA_TOO_EXPENSIVE` — under the automatic engine this means "retry
    /// with the backtracker", which is exactly what the caller does.
    TooExpensive,
    /// The run raised a message; the caller reports it and answers "no match",
    /// which is what every VimL function does once an error is raised.
    Error(&'static str),
}

/// Run the compiled NFA over `text`, looking for the leftmost match at or after
/// `from`. c: `nfa_regexec_both()`.
pub fn exec(prog: &Prog, text: &[char], ic: bool, from: usize) -> Outcome {
    let mut col = from;

    // c: "if (prog->reganch && col > 0) return 0L;" — every path out of the
    // start state begins with `^`/`\%^`, and `NFA_BOL`/`NFA_BOF` only fire at
    // index 0 of the subject, so no later start position can match.
    if prog.reganch && col > 0 {
        return Outcome::NoMatch;
    }

    if prog.regstart != 0 {
        // Skip ahead until a character we know the match must start with.
        // When there is none there is no match.
        match skip_to_start(text, prog.regstart, col, ic) {
            Some(c) => col = c,
            None => return Outcome::NoMatch,
        }

        // If match_text is set it contains the full text that must match.
        // Nothing else to try.
        if let Some(mt) = &prog.match_text {
            if !mt.is_empty() {
                return match find_match_text(text, &mut col, prog.regstart, mt, ic) {
                    Some((s, e)) => {
                        let mut subs = vec![None; prog.nsubexp];
                        subs[0] = Some((s, e));
                        Outcome::Match(NfaMatch { subs })
                    }
                    None => Outcome::NoMatch,
                };
            }
        }
    }

    let from = col;
    let mut m = Matcher::new(prog, text, ic);
    m.input = from;
    let mut subs = RegSub::default();
    let mut mm = RegSub::default();
    let start = prog.start as i32;
    let result = m.regmatch(start, &mut subs, &mut mm);
    if let Some(e) = m.err {
        return Outcome::Error(e);
    }
    match result {
        MatchResult::TooExpensive => Outcome::TooExpensive,
        MatchResult::No => Outcome::NoMatch,
        MatchResult::Yes => {
            let mut out: Vec<Option<(usize, usize)>> = vec![None; prog.nsubexp];
            for (i, slot) in out.iter_mut().enumerate() {
                if i < subs.in_use {
                    if let (Some(s), Some(e)) = (subs.list[i].0.get(), subs.list[i].1.get()) {
                        *slot = Some((s, e));
                    }
                }
            }
            // c: "if (rex.reg_startp[0] == NULL) rex.reg_startp[0] = rex.line + col"
            // and "if (rex.reg_endp[0] == NULL) rex.reg_endp[0] = rex.input".
            let s0 = subs.list[0].0.get().unwrap_or(from);
            let e0 = subs.list[0].1.get().unwrap_or(m.input);
            // c: "Make sure the end is never before the start.  Can happen when
            // \zs and \ze are used."
            out[0] = Some((s0, if e0 < s0 { s0 } else { e0 }));
            Outcome::Match(NfaMatch { subs: out })
        }
    }
}
