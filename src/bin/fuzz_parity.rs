//! `fuzz-parity` — differential fuzzer: vimlrs vs. real Vim/Neovim.
//!
//! EXTENSION — NO `vendor/` counterpart. This is a development tool, not part
//! of the language runtime.
//!
//! ## What it does
//!
//! 1. **Generates** random VimL expressions from a grammar seeded by a 64-bit
//!    seed (reproducible: same seed → same corpus, no `rand` dependency).
//! 2. **Runs each through vimlrs in a child process** (this same binary,
//!    re-executed as `--child`), which evaluates in-process via
//!    [`vimlrs::eval_expr`] under `catch_unwind` and flushes one result line per
//!    expression. Values render with `string()` semantics (`encode_tv2string`);
//!    errors come from the `assert_fails()` capture hook, so nothing reaches the
//!    terminal. The child is capped by [`MEM_LIMIT`] and a wall-clock deadline,
//!    so a panic, a hard crash, a runaway allocation, and an infinite loop are
//!    all *findings* attributed to the exact expression that caused them — the
//!    parent restarts the child after that index and keeps going.
//! 3. **Runs the same corpus through the oracles** — `nvim` and `vim` — by
//!    emitting one driver script per engine that `eval()`s every expression
//!    inside `try`/`catch` and `writefile()`s the results. One process per
//!    engine for the whole corpus, not one per expression.
//! 4. **Triages** each expression by comparing all three results:
//!
//!    | class      | condition                                    | meaning                       |
//!    |------------|----------------------------------------------|-------------------------------|
//!    | `Ok`       | vimlrs == oracle                             | parity                        |
//!    | `Panic`    | vimlrs panicked / crashed / hung             | crash bug (always a finding)  |
//!    | `Gap`      | `nvim == vim` and vimlrs differs             | confirmed parity gap          |
//!    | `Divergent`| `nvim != vim`, vimlrs matches one of them    | Vim/Neovim differ — advisory  |
//!
//!    Only `Gap` and `Panic` are actionable; `Divergent` is reported separately
//!    so a Vim-vs-Neovim behavior split is never mistaken for a vimlrs bug.
//!    An expression the *oracle* couldn't answer (it crashed or hung Vim too —
//!    `range(9223372036854775807)` does exactly that) is `OracleFail`, and is
//!    never counted against vimlrs: with no spec there is nothing to be wrong
//!    about.
//!
//! Errors compare by **E-number only** (`E121`), never by message prose: the
//! number is Vim's stable contract, the wording is not.
//!
//! ## Determinism
//!
//! Every expression is evaluated against a fresh [`PRELUDE`] (the same `g:`
//! variables in every engine, re-established before each expression), so a
//! mutating call like `add(g:l, 4)` can't leak into the next case. The builtin
//! allow-list ([`FUNCS`]) admits only pure, deterministic, non-blocking
//! functions — nothing touching the clock, the filesystem, the process table,
//! the RNG, or an editor buffer.
//!
//! ## Usage
//!
//! ```text
//! cargo run --bin fuzz-parity -- --count 3000 --seed 7
//! cargo run --bin fuzz-parity -- --count 3000 --seed 7 --corpus fuzz_corpus.txt
//! cargo run --bin fuzz-parity -- --only substitute,printf --count 500
//! ```
//!
//! `--corpus FILE` appends every confirmed gap as an oracle-recorded
//! `expr<TAB>expected` line, which is what `tests/fuzz_corpus.rs` replays as a
//! Vim-free CI gate.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use vimlrs::ported::eval::encode::encode_tv2string;
use vimlrs::ported::eval::funcs_argc::BUILTIN_ARGC;
use vimlrs::ported::message::{capture_errors_begin, capture_errors_take};

// ─── Resource guards ────────────────────────────────────────────────────────
// A fuzzer that can take the machine down with it is not a fuzzer. Both sides
// need the same two guards, because *both* engines will happily try to
// materialize `range(9223372036854775807)`.

/// Heap ceiling for a child evaluating expressions. Exceeding it fails the
/// allocation, which Rust turns into an abort — the parent sees the child die,
/// attributes it to the expression in flight, and resumes after it. Without
/// this, a runaway allocation thrashes the machine for a minute before the
/// kernel steps in.
const MEM_LIMIT: usize = 1 << 30; // 1 GiB

/// Wall-clock ceiling per child / per oracle chunk.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(30);

/// Expressions per oracle process. A chunk that dies costs only its own
/// remainder, not the whole corpus.
const CHUNK: usize = 250;

/// Allocator that enforces [`MEM_LIMIT`] once armed. The parent runs unarmed
/// (`LIMIT` = `usize::MAX`); a `--child` arms it before evaluating anything.
struct Budget;

static USED: AtomicUsize = AtomicUsize::new(0);
static LIMIT: AtomicUsize = AtomicUsize::new(usize::MAX);

unsafe impl GlobalAlloc for Budget {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if USED.fetch_add(l.size(), Ordering::Relaxed) + l.size() > LIMIT.load(Ordering::Relaxed) {
            USED.fetch_sub(l.size(), Ordering::Relaxed);
            return std::ptr::null_mut();
        }
        let p = unsafe { System.alloc(l) };
        if p.is_null() {
            USED.fetch_sub(l.size(), Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        USED.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static ALLOC: Budget = Budget;

/// Wait for `child` up to `deadline`, killing it if it overruns. Returns
/// `Ok(true)` when it exited on its own, `Ok(false)` when it had to be killed.
fn wait_bounded(child: &mut Child, deadline: Duration) -> bool {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if start.elapsed() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

// ─── PRNG ───────────────────────────────────────────────────────────────────
// SplitMix64 — 3 lines, no dependency, identical stream on every platform, so a
// seed reported in a bug is reproducible on any machine and in any year.

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (`n > 0`).
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// Pick one element of a non-empty slice.
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }

    /// True with probability `n`/`d`.
    fn chance(&mut self, n: u64, d: u64) -> bool {
        self.next() % d < n
    }
}

// ─── Argument shapes ────────────────────────────────────────────────────────

/// The kind of value a builtin's positional argument wants. Random *typed*
/// arguments keep the corpus in the region where the interesting semantics live
/// (coercion, clamping, empty/negative/out-of-range edges); random *untyped*
/// arguments would mostly reproduce E-number checks that the arity table
/// already gates.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    /// Any string, including empty / unicode / embedded quotes.
    Str,
    /// Integer, weighted toward the edges (0, ±1, INT_MAX, 64-bit extremes).
    Num,
    /// Float, including negative / zero / very large.
    Float,
    /// 0 or 1 (Vim's boolean convention).
    Bool,
    /// A regex pattern — the highest-value shape: Vim's regex dialect is where
    /// a hand-written engine diverges most.
    Pat,
    /// A `printf()` format string.
    Fmt,
    /// A list of mixed values.
    List,
    /// A list of strings (for `join`, `sort`, `filter` over strings).
    StrList,
    /// A list of numbers (for `max`, `min`, `sort` numeric).
    NumList,
    /// A dict.
    Dict,
    /// A blob.
    Blob,
    /// Any value at all (for `type()`, `string()`, `empty()`, …).
    Any,
    /// A lambda expression, `{x -> …}` (for `map`/`filter`/`sort`/`reduce`).
    Lambda,
    /// A `substitute()` replacement string (may carry `\1`, `\=expr`, `~`).
    Repl,
    /// `sort()`/`uniq()` comparison flag: `'i'`, `'n'`, `'N'`, `'l'`, `1`, `0`.
    SortFlag,
    /// A *size*: how many elements/copies to materialize (`range`, `repeat`,
    /// `flatten` depth). Bounded on purpose — `range(9223372036854775807)`
    /// allocates without bound in **real Vim and Neovim too**, so it is a
    /// resource pathology shared by all three engines, not a parity gap, and
    /// generating it only costs a timeout per case.
    Count,
}

use Shape::*;

/// Per-builtin positional argument shapes.
///
/// The *count* of arguments emitted is not taken from this table — it is drawn
/// from [`BUILTIN_ARGC`], the generated arity metadata the compiler itself
/// enforces, so the fuzzer and the interpreter can never disagree about how
/// many arguments a call may carry. This table only says what each slot should
/// *look like*; a call that wants more slots than are listed repeats the last
/// shape.
///
/// The allow-list is deliberate: every function here is pure, deterministic,
/// non-blocking, and independent of the filesystem, clock, environment, RNG,
/// and editor state. Nothing else may be added — an impure builtin would report
/// a false "gap" on every run.
const FUNCS: &[(&str, &[Shape])] = &[
    // ── strings ─────────────────────────────────────────────────────────────
    ("strlen", &[Str]),
    ("strchars", &[Str, Bool]),
    ("strwidth", &[Str]),
    ("strdisplaywidth", &[Str, Num]),
    ("strcharlen", &[Str]),
    ("strpart", &[Str, Num, Num, Bool]),
    ("strcharpart", &[Str, Num, Num]),
    ("strgetchar", &[Str, Num]),
    ("strcharatpos", &[Str, Num]),
    ("stridx", &[Str, Str, Num]),
    ("strridx", &[Str, Str, Num]),
    ("strtrans", &[Str]),
    ("tolower", &[Str]),
    ("toupper", &[Str]),
    ("trim", &[Str, Str, Num]),
    ("repeat", &[Any, Count]),
    ("reverse", &[Any]),
    ("split", &[Str, Pat, Bool]),
    ("join", &[List, Str]),
    ("substitute", &[Str, Pat, Repl, Str]),
    ("escape", &[Str, Str]),
    ("shellescape", &[Str, Bool]),
    ("fnameescape", &[Str]),
    ("printf", &[Fmt, Any, Any, Any]),
    ("nr2char", &[Num, Bool]),
    ("char2nr", &[Str, Bool]),
    ("str2nr", &[Str, Num, Bool]),
    ("str2float", &[Str, Bool]),
    ("str2list", &[Str, Bool]),
    ("list2str", &[NumList, Bool]),
    ("byteidx", &[Str, Num, Bool]),
    ("byteidxcomp", &[Str, Num, Bool]),
    ("charidx", &[Str, Num, Bool]),
    ("strspn", &[Str, Str]),
    ("strcspn", &[Str, Str]),
    ("eval", &[Str]),
    ("string", &[Any]),
    ("iconv", &[Str, Str, Str]),
    ("keytrans", &[Str]),
    // ── regex ───────────────────────────────────────────────────────────────
    ("match", &[Str, Pat, Num, Num]),
    ("matchend", &[Str, Pat, Num, Num]),
    ("matchstr", &[Str, Pat, Num, Num]),
    ("matchstrpos", &[Str, Pat, Num, Num]),
    ("matchlist", &[Str, Pat, Num, Num]),
    ("matchbufline", &[Num, Pat, Num, Num]),
    ("matchfuzzy", &[StrList, Str]),
    // ── numbers / math ──────────────────────────────────────────────────────
    ("abs", &[Float]),
    ("ceil", &[Float]),
    ("floor", &[Float]),
    ("round", &[Float]),
    ("trunc", &[Float]),
    ("float2nr", &[Float]),
    ("fmod", &[Float, Float]),
    ("pow", &[Float, Float]),
    ("sqrt", &[Float]),
    ("exp", &[Float]),
    ("log", &[Float]),
    ("log10", &[Float]),
    ("sin", &[Float]),
    ("cos", &[Float]),
    ("tan", &[Float]),
    ("asin", &[Float]),
    ("acos", &[Float]),
    ("atan", &[Float]),
    ("atan2", &[Float, Float]),
    ("sinh", &[Float]),
    ("cosh", &[Float]),
    ("tanh", &[Float]),
    ("and", &[Num, Num]),
    ("or", &[Num, Num]),
    ("xor", &[Num, Num]),
    ("invert", &[Num]),
    ("max", &[NumList]),
    ("min", &[NumList]),
    ("isinf", &[Float]),
    ("isnan", &[Float]),
    // ── lists / dicts ───────────────────────────────────────────────────────
    ("len", &[Any]),
    ("empty", &[Any]),
    ("get", &[Any, Any, Any]),
    ("add", &[List, Any]),
    ("insert", &[List, Any, Num]),
    ("remove", &[Any, Any, Any]),
    ("extend", &[List, List, Num]),
    ("copy", &[Any]),
    ("deepcopy", &[Any, Bool]),
    ("count", &[Any, Any, Bool, Num]),
    ("index", &[List, Any, Num, Bool]),
    ("indexof", &[List, Lambda]),
    ("map", &[Any, Lambda]),
    ("filter", &[Any, Lambda]),
    ("sort", &[List, SortFlag]),
    ("uniq", &[List, SortFlag]),
    ("reduce", &[List, Lambda, Any]),
    ("flatten", &[List, Count]),
    ("flattennew", &[List, Count]),
    ("range", &[Count, Count, Count]),
    ("keys", &[Dict]),
    ("values", &[Dict]),
    ("items", &[Dict]),
    ("has_key", &[Dict, Str]),
    ("zip", &[List, List]),
    // ── blobs ───────────────────────────────────────────────────────────────
    ("blob2list", &[Blob]),
    ("list2blob", &[NumList]),
    ("blob2str", &[Blob]),
    ("str2blob", &[StrList]),
    // ── types / conversion ──────────────────────────────────────────────────
    ("type", &[Any]),
    ("typename", &[Any]),
    ("json_encode", &[Any]),
    ("json_decode", &[Str]),
    ("msgpackdump", &[List]),
    ("id", &[Any]),
];

// ─── Value pools ────────────────────────────────────────────────────────────
// Edge-weighted literals. Every entry is a VimL source fragment.

const STRINGS: &[&str] = &[
    "''",
    "'a'",
    "'abc'",
    "'ABC'",
    "'hello world'",
    "'  padded  '",
    "'a,b,,c'",
    "'foo.bar'",
    "'1234'",
    "'-7'",
    "'3.5e2'",
    "'0x1f'",
    "'tab\\there'",
    "'ünïcø∂é'",
    "'日本語'",
    "'e\u{0301}combining'",
    "'a''quote'",
    "\"dq\\tstr\"",
    "\"nl\\nhere\"",
    "'*.[]^$\\'",
    "'\\d\\+'",
];

const NUMS: &[&str] = &[
    "0",
    "1",
    "2",
    "3",
    "-1",
    "-2",
    "7",
    "10",
    "-10",
    "255",
    "256",
    "-255",
    "2147483647",
    "-2147483648",
    "9223372036854775807",
    "-9223372036854775808",
    "0x1f",
    "0b1011",
    "017",
    "100000",
];

/// Bounded sizes for [`Shape::Count`] slots — still covering the interesting
/// edges (zero, negative, one-past) without asking any engine to materialize a
/// 9-quintillion-element list.
const COUNTS: &[&str] = &["0", "1", "2", "3", "5", "10", "64", "1000", "-1", "-3"];

const FLOATS: &[&str] = &[
    "0.0",
    "1.0",
    "-1.0",
    "0.5",
    "-0.5",
    "1.5",
    "2.75",
    "-3.25",
    "3.141592653589793",
    "1.0e10",
    "1.0e-10",
    "-1.0e300",
    "1.0e308",
    "123456789.123456789",
    "0.1",
];

/// Regex patterns in Vim's dialect — magic atoms, quantifiers, classes,
/// anchors, zero-width bounds, alternation, lookaround, POSIX classes.
const PATS: &[&str] = &[
    "'a'",
    "'.'",
    "'.*'",
    "'^a'",
    "'c$'",
    "'\\.'",
    "'a\\+'",
    "'a\\='",
    "'a\\?'",
    "'a\\{2}'",
    "'a\\{1,3}'",
    "'a\\{-}'",
    "'a\\{-1,}'",
    "'\\d'",
    "'\\d\\+'",
    "'\\D'",
    "'\\w\\+'",
    "'\\W'",
    "'\\s'",
    "'\\S\\+'",
    "'\\a'",
    "'\\l'",
    "'\\u'",
    "'\\x'",
    "'\\o'",
    "'\\h'",
    "'\\k'",
    "'\\p'",
    "'[abc]'",
    "'[^abc]'",
    "'[a-z]\\+'",
    "'[[:digit:]]\\+'",
    "'[[:alpha:]]'",
    "'[[:space:]]'",
    "'[[:punct:]]'",
    "'\\(a\\)\\(b\\)'",
    "'\\(ab\\)\\1'",
    "'a\\|b'",
    "'\\%(ab\\)\\+'",
    "'\\zsa'",
    "'a\\zs.'",
    "'.\\zea'",
    "'\\<a'",
    "'a\\>'",
    "'\\ca'",
    "'\\Ca'",
    "'\\vA+'",
    "'\\v(a|b)+'",
    "'\\V.'",
    "'\\Ma.'",
    "'a\\@='",
    "'a\\@!'",
    "'\\(a\\)\\@<=b'",
    "'\\(a\\)\\@<!b'",
    "'\\%[abc]'",
    "'\\%d97'",
    "'\\%x61'",
    "'\\_.'",
    "'\\n'",
    "'\\t'",
    "''",
];

/// `printf()` formats: every conversion vimlrs claims to support, plus the
/// width/precision/flag combinations that are easy to get subtly wrong.
const FMTS: &[&str] = &[
    "'%d'",
    "'%5d'",
    "'%-5d|'",
    "'%05d'",
    "'%+d'",
    "'% d'",
    "'%x'",
    "'%X'",
    "'%#x'",
    "'%o'",
    "'%b'",
    "'%c'",
    "'%s'",
    "'%10s|'",
    "'%-10s|'",
    "'%.3s'",
    "'%S'",
    "'%f'",
    "'%.2f'",
    "'%e'",
    "'%E'",
    "'%g'",
    "'%G'",
    "'%%'",
    "'%*d'",
    "'%.*f'",
    "'%1$s %1$s'",
    "'a%db%sc'",
    "'%s=%d'",
];

const LISTS: &[&str] = &[
    "[]",
    "[1]",
    "[1,2,3]",
    "[3,1,2]",
    "[1,1,2,2,3]",
    "['a','b','c']",
    "['b','a','C','A']",
    "[1,'a',2.5]",
    "[[1,2],[3,4]]",
    "[[1,[2,[3]]]]",
    "[{'a':1},{'a':2}]",
    "[0,-1,255]",
    "[v:true,v:false,v:null]",
    "['10','9','2']",
];

const STRLISTS: &[&str] = &[
    "[]",
    "['a']",
    "['a','b','c']",
    "['foo','bar','baz']",
    "['B','a','C']",
    "['','x','']",
];

const NUMLISTS: &[&str] = &[
    "[]",
    "[0]",
    "[1,2,3]",
    "[3,1,2]",
    "[-1,0,1]",
    "[65,66,67]",
    "[255,256,0]",
    "[97,0x1f,10]",
];

const DICTS: &[&str] = &[
    "{}",
    "{'a':1}",
    "{'a':1,'b':2}",
    "{'b':2,'a':1}",
    "{'a':[1,2],'b':{'c':3}}",
    "{'1':'x','2':'y'}",
    "#{a:1,b:2}",
];

const BLOBS: &[&str] = &["0z", "0z00", "0zFF", "0z0011DEADBEEF", "0z61.62.63"];

const SPECIALS: &[&str] = &["v:true", "v:false", "v:null", "v:none"];

/// Replacement strings for `substitute()` — backrefs, whole-match `&`, `~`, the
/// `\=` expression form, and case-folding escapes.
const REPLS: &[&str] = &[
    "'X'",
    "''",
    "'[&]'",
    "'\\0\\0'",
    "'\\1'",
    "'<\\1>'",
    "'\\u&'",
    "'\\U&'",
    "'\\l&'",
    "'\\e'",
    "'\\=submatch(0)'",
    "'\\=toupper(submatch(0))'",
    "'\\=submatch(0).submatch(0)'",
    "'\\=len(submatch(0))'",
    "'\\n'",
    "'\\r'",
    "'~'",
];

const LAMBDAS: &[&str] = &[
    "{i,v -> v}",
    "{i,v -> i}",
    "{_,v -> v * 2}",
    "{_,v -> type(v)}",
    "{_,v -> string(v)}",
    "{_,v -> v is v}",
    "{i,v -> i % 2}",
    "{_,v -> empty(v)}",
];

/// Comparators for `sort()`/`uniq()` (the 2nd argument).
const SORT_FLAGS: &[&str] = &[
    "'i'",
    "'n'",
    "'N'",
    "'l'",
    "1",
    "0",
    "{a,b -> a > b ? -1 : 1}",
];

/// Global variables the [`PRELUDE`] defines in every engine before each
/// expression, so a generated expression can name (and mutate) a value.
const VARS: &[&str] = &["g:n", "g:f", "g:s", "g:l", "g:d", "g:b", "g:e"];

/// Re-established before *every* expression in every engine, so a mutating call
/// (`add`, `remove`, `sort`, `map`, …) starts from identical state and can't
/// leak into the next case.
const PRELUDE: &[&str] = &[
    "let g:n = 42",
    "let g:f = 2.5",
    "let g:s = 'hello'",
    "let g:l = [1, 2, 3]",
    "let g:d = {'a': 1, 'b': 2}",
    "let g:b = 0z0011",
    "let g:e = []",
];

/// Binary operators. Comparison operators appear in all three case forms
/// (bare / `#` match-case / `?` ignore-case) because that family has regressed
/// before (BUGS.md R1-1).
const BINOPS: &[&str] = &[
    "+", "-", "*", "/", "%", ".", "..", "&&", "||", "==", "!=", ">", ">=", "<", "<=", "=~", "!~",
    "==#", "!=#", ">#", "<#", "=~#", "!~#", "==?", "!=?", ">?", "<?", "=~?", "!~?", "is", "isnot",
];

// ─── Generator ──────────────────────────────────────────────────────────────

/// One value of the given shape.
fn shaped(rng: &mut Rng, s: Shape) -> String {
    match s {
        Str => rng.pick(STRINGS).to_string(),
        Num => rng.pick(NUMS).to_string(),
        Float => rng.pick(FLOATS).to_string(),
        Bool => ["0", "1"][rng.below(2)].to_string(),
        Pat => rng.pick(PATS).to_string(),
        Fmt => rng.pick(FMTS).to_string(),
        List => rng.pick(LISTS).to_string(),
        StrList => rng.pick(STRLISTS).to_string(),
        NumList => rng.pick(NUMLISTS).to_string(),
        Dict => rng.pick(DICTS).to_string(),
        Blob => rng.pick(BLOBS).to_string(),
        Lambda => rng.pick(LAMBDAS).to_string(),
        Repl => rng.pick(REPLS).to_string(),
        SortFlag => rng.pick(SORT_FLAGS).to_string(),
        Count => rng.pick(COUNTS).to_string(),
        Any => any_value(rng),
    }
}

/// A value of *any* type — the pool that exercises coercion and `E7xx` type
/// errors.
fn any_value(rng: &mut Rng) -> String {
    match rng.below(8) {
        0 => rng.pick(NUMS).to_string(),
        1 => rng.pick(STRINGS).to_string(),
        2 => rng.pick(FLOATS).to_string(),
        3 => rng.pick(LISTS).to_string(),
        4 => rng.pick(DICTS).to_string(),
        5 => rng.pick(BLOBS).to_string(),
        6 => rng.pick(SPECIALS).to_string(),
        _ => rng.pick(VARS).to_string(),
    }
}

/// Accepted argument-count range for `name`, straight from the arity table the
/// compiler enforces.
fn argc_range(name: &str) -> (u8, u8) {
    BUILTIN_ARGC
        .binary_search_by_key(&name, |(n, _, _)| n)
        .map(|i| (BUILTIN_ARGC[i].1, BUILTIN_ARGC[i].2))
        .unwrap_or((0, 0))
}

/// A call to one allow-listed builtin, with an argument count drawn from the
/// real arity range and typed arguments from its shape row.
fn gen_call(rng: &mut Rng, funcs: &[(&str, &[Shape])]) -> String {
    let (name, shapes) = *rng.pick(funcs);
    let (min, max) = argc_range(name);
    // Cap the varargs sentinel (255) at the shapes we have; `printf` is the only
    // real varargs case and its row already lists the slots worth filling.
    let hi = (max as usize).min(shapes.len().max(min as usize));
    let n = if hi > min as usize {
        min as usize + rng.below(hi - min as usize + 1)
    } else {
        min as usize
    };
    let args: Vec<String> = (0..n)
        .map(|i| shaped(rng, shapes[i.min(shapes.len() - 1)]))
        .collect();
    format!("{name}({})", args.join(","))
}

/// One expression, recursively. `depth` bounds nesting so the corpus stays
/// human-readable when a case has to be pasted into a bug report.
fn gen_expr(rng: &mut Rng, funcs: &[(&str, &[Shape])], depth: u32) -> String {
    if depth == 0 {
        return match rng.below(4) {
            0 => gen_call(rng, funcs),
            _ => any_value(rng),
        };
    }
    match rng.below(10) {
        // Builtin call — the bulk of the corpus.
        0..=4 => {
            let call = gen_call(rng, funcs);
            // Sometimes index or slice the result: `split(…)[0]`, `s[1:3]`.
            if rng.chance(1, 5) {
                let i = rng.pick(&["0", "1", "-1", "2", "5", "-5"]);
                if rng.chance(1, 2) {
                    format!("{call}[{i}]")
                } else {
                    let j = rng.pick(&["", "0", "1", "-1", "3"]);
                    format!("{call}[{i}:{j}]")
                }
            } else {
                call
            }
        }
        // Binary operator over two sub-expressions.
        5..=7 => {
            let a = gen_expr(rng, funcs, depth - 1);
            let b = gen_expr(rng, funcs, depth - 1);
            let op = rng.pick(BINOPS);
            // `.` needs surrounding space to stay concatenation, not a member
            // read (BUGS.md R4-1); `..` and the word operators need it too.
            format!("({a} {op} {b})")
        }
        // Unary.
        8 => {
            let a = gen_expr(rng, funcs, depth - 1);
            let op = rng.pick(&["!", "-", "+"]);
            format!("{op}({a})")
        }
        // Ternary.
        _ => {
            let c = gen_expr(rng, funcs, depth - 1);
            let a = gen_expr(rng, funcs, depth - 1);
            let b = gen_expr(rng, funcs, depth - 1);
            format!("({c} ? {a} : {b})")
        }
    }
}

// ─── Results ────────────────────────────────────────────────────────────────

/// The outcome of evaluating one expression in one engine.
#[derive(Clone, PartialEq, Eq)]
enum Outcome {
    /// `string()` of the value.
    Val(String),
    /// The E-number only (`E121`) — the message prose is not a contract.
    Err(String),
    /// vimlrs panicked (never a legal outcome).
    Panic(String),
    /// The oracle produced no line for this index (it died or hung).
    Missing,
}

impl Outcome {
    fn show(&self) -> String {
        match self {
            Outcome::Val(v) => v.clone(),
            Outcome::Err(e) => format!("<{e}>"),
            Outcome::Panic(p) => format!("<PANIC: {p}>"),
            Outcome::Missing => "<none>".into(),
        }
    }
}

/// Pull the first `E<digits>` out of an error message; Vim's message text is
/// not stable across versions but the number is.
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

/// Evaluate one expression in-process, with the [`PRELUDE`] re-established
/// first. Panics are caught and reported rather than killing the run.
fn eval_one(expr: &str) -> Outcome {
    let r = panic::catch_unwind(AssertUnwindSafe(|| {
        capture_errors_begin();
        for line in PRELUDE {
            let _ = vimlrs::eval_source(line);
        }
        let _ = capture_errors_take();

        capture_errors_begin();
        let out = vimlrs::eval_expr(expr);
        let errs = capture_errors_take();
        match out {
            // An expression can produce a value *and* have raised an error
            // (Vim's evaluator recovers with an empty value); the error is the
            // observable outcome, so it wins.
            Ok(v) if errs.is_empty() => Outcome::Val(encode_tv2string(&v)),
            Ok(_) => Outcome::Err(enumber(&errs[0])),
            Err(e) => Outcome::Err(enumber(&e.to_string())),
        }
    }));
    r.unwrap_or_else(|e| {
        let msg = e
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| e.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "?".into());
        Outcome::Panic(format!("panic: {}", msg.lines().next().unwrap_or("?")))
    })
}

/// `--child CORPUS START OUT`: evaluate `CORPUS` from line `START` to the end,
/// appending one flushed result line per expression to `OUT`. Runs under the
/// [`MEM_LIMIT`] budget, so a runaway allocation aborts *this* process only.
///
/// The flush-per-line is what lets the parent attribute a hard death (abort,
/// SIGSEGV, OOM kill, timeout) to an exact expression: the first index with no
/// line is the one that killed the child.
fn child_main(corpus: &Path, start: usize, out: &Path) -> ! {
    LIMIT.store(MEM_LIMIT, Ordering::Relaxed);
    panic::set_hook(Box::new(|_| {}));

    let text = std::fs::read_to_string(corpus).expect("corpus");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out)
        .expect("outcomes");

    for (i, expr) in text.lines().enumerate().skip(start) {
        let (tag, payload) = match eval_one(expr) {
            Outcome::Val(v) => ("V", v),
            Outcome::Err(e) => ("E", e),
            Outcome::Panic(p) => ("P", p),
            Outcome::Missing => ("P", "vanished".into()),
        };
        // One line, one flush — a crash on the *next* expression must not lose
        // the answer to this one. A value can legally contain a newline
        // (`nr2char(10)`), which would split the record; Vim's `writefile()`
        // encodes an embedded newline as NUL, so encode it the same way here and
        // the two transports stay byte-comparable.
        let _ = writeln!(f, "{i}\t{tag}\t{}", payload.replace('\n', "\0"));
        let _ = f.flush();
    }
    std::process::exit(0);
}

/// Read a result file as *bytes*, lossily decoded.
///
/// Never `read_to_string`: an engine's output is not necessarily valid UTF-8
/// (`nr2char(200)`, `blob2str(0zFF)`, and Vim's NUL-for-newline encoding all
/// produce non-UTF-8 bytes), and a strict decode would throw away every result
/// in the file — which silently looks like "the oracle answered nothing".
fn slurp(path: &Path) -> String {
    std::fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

/// Parse an outcomes file (child protocol) into `res`.
fn read_outcomes(path: &Path, res: &mut [Outcome]) {
    let text = slurp(path);
    for line in text.lines() {
        let mut it = line.splitn(3, '\t');
        let (Some(i), Some(tag), Some(payload)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let Ok(i) = i.parse::<usize>() else { continue };
        if i >= res.len() {
            continue;
        }
        res[i] = match tag {
            "V" => Outcome::Val(payload.to_string()),
            "E" => Outcome::Err(payload.to_string()),
            _ => Outcome::Panic(payload.to_string()),
        };
    }
}

/// Run the whole corpus through vimlrs in child processes, restarting after any
/// expression that kills or hangs a child and recording it as a crash finding.
fn run_vimlrs(exprs: &[String], tmp: &Path) -> Vec<Outcome> {
    let corpus = tmp.join("corpus.txt");
    let out = tmp.join("outcomes.txt");
    std::fs::write(&corpus, format!("{}\n", exprs.join("\n"))).expect("write corpus");
    let _ = std::fs::remove_file(&out);

    let me = std::env::current_exe().expect("current exe");
    let mut res = vec![Outcome::Missing; exprs.len()];
    let mut next = 0usize;

    while next < exprs.len() {
        let mut child = Command::new(&me)
            .arg("--child")
            .arg(&corpus)
            .arg(next.to_string())
            .arg(&out)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn child");
        let finished = wait_bounded(&mut child, CHUNK_TIMEOUT);
        read_outcomes(&out, &mut res);

        // First still-unanswered index: the expression the child died on.
        let Some(stuck) = (next..exprs.len()).find(|&i| res[i] == Outcome::Missing) else {
            break;
        };
        if finished && stuck == exprs.len() {
            break;
        }
        res[stuck] = Outcome::Panic(if finished {
            "crashed (abort / OOM / signal)".into()
        } else {
            format!("hung (>{}s)", CHUNK_TIMEOUT.as_secs())
        });
        next = stuck + 1;
    }
    res
}

/// Quote a generated expression as a single-quoted VimL string literal (the
/// only quoting form with no escape sequences: `''` is the sole escape).
fn vim_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Build the driver script an oracle runs for `exprs[lo..hi]`: re-establish the
/// prelude, `eval()` each expression inside `try`/`catch`, and append the
/// `string()` of the value (or the exception) to `out_path`.
///
/// Results are written with `writefile(…, 'a')` *per expression*, not batched at
/// the end, for the same reason the child flushes per line: an expression that
/// kills or hangs the editor must not take the answers before it down too.
fn driver(exprs: &[String], lo: usize, hi: usize, out_path: &Path) -> String {
    let mut s = String::new();
    s.push_str("set cpo&vim\n");
    let _ = writeln!(
        s,
        "let s:exprs = [{}]",
        exprs[lo..hi]
            .iter()
            .map(|e| vim_quote(e))
            .collect::<Vec<_>>()
            .join(",")
    );
    let _ = writeln!(
        s,
        "let s:out = {}",
        vim_quote(&out_path.display().to_string())
    );
    let _ = writeln!(s, "for s:i in range({lo}, {})", hi - 1);
    for line in PRELUDE {
        let _ = writeln!(s, "  {line}");
    }
    s.push_str("  try\n");
    let _ = writeln!(
        s,
        "    let s:r = s:i . \"\\tV\\t\" . string(eval(s:exprs[s:i - {lo}]))"
    );
    s.push_str("  catch\n");
    s.push_str("    let s:r = s:i . \"\\tE\\t\" . v:exception\n");
    s.push_str("  endtry\n");
    s.push_str("  call writefile([s:r], s:out, 'a')\n");
    s.push_str("endfor\n");
    s.push_str("qa!\n");
    s
}

/// Read an oracle's output file into `res` (same three-field protocol as the
/// child, except the payload of an `E` line is the raw exception, from which
/// only the E-number is kept).
fn read_oracle(path: &Path, res: &mut [Outcome]) {
    let text = slurp(path);
    for line in text.lines() {
        let mut it = line.splitn(3, '\t');
        let (Some(i), Some(tag), Some(payload)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let Ok(i) = i.parse::<usize>() else { continue };
        if i >= res.len() {
            continue;
        }
        res[i] = match tag {
            "V" => Outcome::Val(payload.to_string()),
            "E" => Outcome::Err(enumber(payload)),
            _ => continue,
        };
    }
}

/// Run one oracle over the corpus in [`CHUNK`]-sized processes, bounded by
/// [`CHUNK_TIMEOUT`]. An expression that kills or hangs the editor
/// (`range(9223372036854775807)` hangs both of them) stays [`Outcome::Missing`]
/// and the next process resumes right after it — one pathological case costs
/// one expression, not the rest of the run.
fn run_oracle(engine: &str, exprs: &[String], tmp: &Path) -> Vec<Outcome> {
    let script = tmp.join(format!("drv_{engine}.vim"));
    let out = tmp.join(format!("out_{engine}.txt"));
    let _ = std::fs::remove_file(&out);

    let mut res = vec![Outcome::Missing; exprs.len()];
    let mut next = 0usize;

    while next < exprs.len() {
        let hi = (next + CHUNK).min(exprs.len());
        std::fs::write(&script, driver(exprs, next, hi, &out)).expect("write driver");

        let mut cmd = Command::new(engine);
        if engine == "nvim" {
            cmd.args(["--headless", "--clean", "-S"]).arg(&script);
        } else {
            cmd.args(["-es", "-u", "NONE", "-i", "NONE", "-S"])
                .arg(&script);
        }
        let spawned = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = spawned else {
            return res; // engine not installed — caller reports it
        };
        wait_bounded(&mut child, CHUNK_TIMEOUT);
        read_oracle(&out, &mut res);

        // Skip past whatever the editor choked on and carry on from there.
        next = match (next..hi).find(|&i| res[i] == Outcome::Missing) {
            Some(stuck) => stuck + 1,
            None => hi,
        };
    }
    res
}

// ─── Triage ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Ok,
    Gap,
    Panic,
    Divergent,
    OracleFail,
}

/// Classify one expression from the three engines' outcomes.
fn classify(v: &Outcome, nv: &Outcome, vi: &Outcome) -> Class {
    if matches!(v, Outcome::Panic(_)) {
        return Class::Panic;
    }
    match (nv, vi) {
        (Outcome::Missing, _) | (_, Outcome::Missing) => Class::OracleFail,
        // Both oracles agree: that is the spec, and vimlrs either matches it or
        // has a gap.
        (a, b) if a == b => {
            if v == a {
                Class::Ok
            } else {
                Class::Gap
            }
        }
        // Vim and Neovim genuinely differ here. vimlrs ports the *Neovim* eval
        // engine, so matching nvim is parity; matching neither is still only
        // advisory, because there is no single spec to be wrong about.
        _ => {
            if v == nv {
                Class::Ok
            } else {
                Class::Divergent
            }
        }
    }
}

/// Bucket key for deduplication: the leading builtin name (or `<expr>` for a
/// pure-operator case) plus the two outcome kinds. Hundreds of instances of one
/// bug collapse to one report line with a count.
fn signature(expr: &str, v: &Outcome, o: &Outcome) -> String {
    let head: String = expr
        .trim_start_matches(['(', '!', '-', '+'])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let name = if head.is_empty() {
        "<expr>".to_string()
    } else {
        head
    };
    let kind = |o: &Outcome| match o {
        Outcome::Val(_) => "val".to_string(),
        Outcome::Err(e) => e.clone(),
        Outcome::Panic(_) => "panic".into(),
        Outcome::Missing => "none".into(),
    };
    format!("{name} [{} vs {}]", kind(v), kind(o))
}

// ─── main ───────────────────────────────────────────────────────────────────

struct Args {
    count: usize,
    seed: u64,
    depth: u32,
    only: Vec<String>,
    corpus: Option<String>,
    verbose: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        count: 1000,
        seed: 1,
        depth: 2,
        only: Vec::new(),
        corpus: None,
        verbose: false,
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        let next = |i: usize| argv.get(i + 1).cloned().unwrap_or_default();
        match argv[i].as_str() {
            "--count" | "-n" => {
                a.count = next(i).parse().unwrap_or(a.count);
                i += 1;
            }
            "--seed" | "-s" => {
                a.seed = next(i).parse().unwrap_or(a.seed);
                i += 1;
            }
            "--depth" | "-d" => {
                a.depth = next(i).parse().unwrap_or(a.depth);
                i += 1;
            }
            "--only" => {
                a.only = next(i).split(',').map(str::to_string).collect();
                i += 1;
            }
            "--corpus" => {
                a.corpus = Some(next(i));
                i += 1;
            }
            "--verbose" | "-v" => a.verbose = true,
            "--help" | "-h" => {
                println!(
                    "fuzz-parity — differential fuzzer: vimlrs vs nvim + vim\n\n\
                     USAGE: fuzz-parity [--count N] [--seed S] [--depth D]\n\
                     \x20                [--only fn1,fn2] [--corpus FILE] [--verbose]\n\n\
                     --count N     expressions to generate (default 1000)\n\
                     --seed S      PRNG seed; same seed → same corpus (default 1)\n\
                     --depth D     max expression nesting depth (default 2)\n\
                     --only LIST   restrict generation to these builtins\n\
                     --corpus FILE append confirmed gaps as `expr<TAB>expected` lines\n\
                     --verbose     list every case, not just divergences"
                );
                std::process::exit(0);
            }
            _ => {}
        }
        i += 1;
    }
    a
}

fn main() {
    // `--child CORPUS START OUT` — the worker half; never used by hand.
    let argv: Vec<String> = std::env::args().collect();
    if argv.get(1).map(String::as_str) == Some("--child") {
        let corpus = PathBuf::from(&argv[2]);
        let start: usize = argv[3].parse().expect("child start index");
        let out = PathBuf::from(&argv[4]);
        child_main(&corpus, start, &out);
    }

    let args = parse_args();

    let funcs: Vec<(&str, &[Shape])> = if args.only.is_empty() {
        FUNCS.to_vec()
    } else {
        FUNCS
            .iter()
            .filter(|(n, _)| args.only.iter().any(|o| o == n))
            .copied()
            .collect()
    };
    if funcs.is_empty() {
        eprintln!("fuzz-parity: --only matched no allow-listed builtin");
        std::process::exit(2);
    }

    // Generate.
    let mut rng = Rng(args.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut exprs: Vec<String> = Vec::with_capacity(args.count);
    while exprs.len() < args.count {
        let e = gen_expr(&mut rng, &funcs, args.depth);
        // A newline or NUL in a generated literal would break the line-oriented
        // oracle transport, not the interpreter — drop those rather than report
        // a transport artifact as a bug.
        if !e.contains('\n') && !e.contains('\0') {
            exprs.push(e);
        }
    }
    eprintln!(
        "fuzz-parity: {} exprs, seed {}, depth {}",
        exprs.len(),
        args.seed,
        args.depth
    );

    let tmp = std::env::temp_dir().join("vimlrs-fuzz");
    std::fs::create_dir_all(&tmp).expect("tmp dir");

    // vimlrs, in child processes (crash/hang/OOM are findings, not lost runs).
    let mine = run_vimlrs(&exprs, &tmp);

    // Oracles, chunked processes with the same guards.
    let nv = run_oracle("nvim", &exprs, &tmp);
    let vi = run_oracle("vim", &exprs, &tmp);
    if nv.iter().all(|o| *o == Outcome::Missing) {
        eprintln!("fuzz-parity: nvim produced no results — is it installed?");
    }
    if vi.iter().all(|o| *o == Outcome::Missing) {
        eprintln!("fuzz-parity: vim produced no results — is it installed?");
    }

    // Triage, deduplicated by signature.
    let mut buckets: BTreeMap<(u8, String), (usize, String, String, String)> = BTreeMap::new();
    let mut counts = [0usize; 5];
    let mut corpus_lines: Vec<String> = Vec::new();

    for (i, e) in exprs.iter().enumerate() {
        let class = classify(&mine[i], &nv[i], &vi[i]);
        let slot = match class {
            Class::Ok => 0,
            Class::Gap => 1,
            Class::Panic => 2,
            Class::Divergent => 3,
            Class::OracleFail => 4,
        };
        counts[slot] += 1;
        if args.verbose {
            println!(
                "[{}] {e}\n    viml={} nvim={} vim={}",
                match class {
                    Class::Ok => "ok",
                    Class::Gap => "GAP",
                    Class::Panic => "PANIC",
                    Class::Divergent => "div",
                    Class::OracleFail => "oracle-fail",
                },
                mine[i].show(),
                nv[i].show(),
                vi[i].show()
            );
        }
        if matches!(class, Class::Ok | Class::OracleFail) {
            continue;
        }
        // Freeze confirmed gaps (and only those) into the replay corpus, with
        // the oracle's answer as the expectation.
        if class == Class::Gap {
            if let Outcome::Val(want) = &nv[i] {
                corpus_lines.push(format!("{e}\t{want}"));
            } else if let Outcome::Err(want) = &nv[i] {
                corpus_lines.push(format!("{e}\t!{want}"));
            }
        }
        let sig = signature(e, &mine[i], &nv[i]);
        let entry = buckets.entry((slot as u8, sig)).or_insert_with(|| {
            (
                0,
                e.clone(),
                mine[i].show(),
                format!("nvim={} vim={}", nv[i].show(), vi[i].show()),
            )
        });
        entry.0 += 1;
    }

    // Report.
    let heading = |slot: u8| match slot {
        1 => "CONFIRMED GAPS (nvim == vim, vimlrs differs)",
        2 => "PANICS (vimlrs crashed)",
        3 => "VIM/NEOVIM DIVERGENCE (advisory — no single spec)",
        _ => "OTHER",
    };
    let mut last = 0u8;
    for ((slot, sig), (n, expr, got, want)) in &buckets {
        if *slot != last {
            println!("\n══ {} ══", heading(*slot));
            last = *slot;
        }
        println!("\n  {sig}  ×{n}");
        println!("    expr:   {expr}");
        println!("    vimlrs: {got}");
        println!("    oracle: {want}");
    }

    println!(
        "\n── summary ──\n  ok:          {}\n  GAPS:        {} ({} distinct)\n  PANICS:      {}\n  divergent:   {}\n  oracle-fail: {}",
        counts[0],
        counts[1],
        buckets.keys().filter(|(s, _)| *s == 1).count(),
        counts[2],
        counts[3],
        counts[4]
    );

    if let Some(path) = &args.corpus {
        if !corpus_lines.is_empty() {
            corpus_lines.sort();
            corpus_lines.dedup();
            let prev = std::fs::read_to_string(path).unwrap_or_default();
            let mut all: Vec<&str> = prev
                .lines()
                .chain(corpus_lines.iter().map(String::as_str))
                .collect();
            all.sort_unstable();
            all.dedup();
            std::fs::write(path, format!("{}\n", all.join("\n"))).expect("write corpus");
            println!("  corpus:      {} cases → {path}", all.len());
        }
    }

    // Exit non-zero when there is something to fix, so the harness can gate.
    if counts[1] > 0 || counts[2] > 0 {
        std::process::exit(1);
    }
}
