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
//!    | `Neither`  | `nvim != vim` and vimlrs matches *neither*   | parity gap (always a finding) |
//!    | `Divergent`| `nvim != vim`, vimlrs matches one of them    | Vim/Neovim differ — advisory  |
//!
//!    `Gap`, `Panic` and `Neither` are actionable; only `Divergent` is advisory,
//!    so a Vim-vs-Neovim behavior split is never mistaken for a vimlrs bug.
//!    An expression the *oracle* couldn't answer (it crashed or hung Vim too —
//!    `range(9223372036854775807)` does exactly that) is `OracleFail`, and is
//!    never counted against vimlrs: with no spec there is nothing to be wrong
//!    about.
//!
//!    **`Neither` exists because "the oracles disagree" is not a reason to stop
//!    looking.** It used to be folded into `Divergent`, and that hid a real bug:
//!    `reverse(10)` returned `0` where vim raises `E1253` and neovim `E1252` —
//!    filed as advisory *because the oracles disagreed*, when in fact vimlrs
//!    agreed with neither and was simply missing an argument check (BUGS.md
//!    R21-7). When the two references disagree they still bracket the answer;
//!    landing outside the bracket is a gap no matter how they split.
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

use vimlrs::fusevm_bridge::{observe_errors_begin, observe_errors_take};
use vimlrs::ported::eval::encode::encode_tv2string;
use vimlrs::ported::eval::funcs_argc::BUILTIN_ARGC;
use vimlrs::ported::message::did_emsg;

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

/// Candidates evaluated per shrink round. Three process spawns cover the whole
/// batch, so this is a memory/latency bound rather than a per-case cost.
const SHRINK_BUDGET: usize = 250;

/// Shrink rounds per finding, before the current best is reported as-is.
const SHRINK_ROUNDS: usize = 12;

/// Wall-clock ceiling per finding. Reduction is a convenience, not a result: a
/// finding that will not shrink inside this is reported at whatever size it
/// reached, rather than holding the whole report hostage.
const SHRINK_TIME: Duration = Duration::from_secs(25);

/// Distinct findings shrunk per run, largest-first is not worth the machinery —
/// the report simply stops reducing after this many and says so.
const SHRINK_MAX_FINDINGS: usize = 40;

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

// ─── Oracle entry points ────────────────────────────────────────────────────
//
// THE ENTRY POINT DECIDES THE ANSWER, NOT JUST THE BINARY. `vim` is not one
// oracle; it is one binary with several dialects selected by the flag vector,
// and the fuzzer compares vimlrs against whichever one it happened to launch.
//
// Measured on this machine (vim 9.2, patches 1-1000), replaying a driver of
// bare `&option` reads through the two entry points this file used to contain:
//
//   -es -u NONE -i NONE     -S DRV   (what run_oracle launched)
//   -es -u NONE -i NONE -N  -S DRV   (what run_program_vim and parity.sh launch)
//
// 14 observables move between them — `&compatible` (1 vs 0), `&fileformats`
// ('' vs 'unix,dos'), `&backspace`, `&whichwrap`, `&history` (0 vs 200),
// `&viminfo`, `&formatoptions` ('vt' vs 'tcq'), `&modeline`, `&shortmess`,
// `&more`, `&ruler`, `&showcmd`, `&hlsearch`, and `&cedit` (which renders the
// same but is a different byte). The driver's own `set cpo&vim` masked
// 'cpoptions' and nothing else, so the rest were live the whole time.
//
// Two entry points inside one fuzzer is worse than either alone: a finding from
// the expression modes and a finding from `--dap` were being reported against
// different reference editors under the same name. Both now come from
// [`Oracle::vim`], which is also parity.sh's vector.
//
// The pin is only worth anything if it is CHECKED, so every oracle is
// preflighted (see [`Oracle::preflight`]) before a corpus is run through it,
// and the resolved absolute path, version line and full argv are printed. A
// harness that cannot name its own oracle is not a differential harness.

/// A reference editor, pinned to one absolute binary and one flag vector.
struct Oracle {
    /// `vim` / `nvim` — how findings name it.
    name: &'static str,
    /// Absolute, symlink-resolved path. `Command::new("vim")` re-resolves PATH
    /// per spawn, so a `brew upgrade` mid-run would silently swap oracles.
    bin: PathBuf,
    /// First line of `--version`, reported with every finding.
    version: String,
    /// Everything before the script path. Printed verbatim; see the note above.
    flags: &'static [&'static str],
    /// What `&compatible` must read as under `flags`, or `None` when the editor
    /// has no such option (Neovim). A mismatch aborts the run.
    want_compatible: Option<i64>,
}

/// vim's pinned vector. `-es` is silent Ex mode; `-u NONE` skips every vimrc;
/// `-i NONE` skips viminfo — WITHOUT it a nocompatible vim also *writes*
/// `~/.viminfo`, so one fuzz run would mutate what the next one reads; `-N` is
/// 'nocompatible', the dialect this crate ports.
const VIM_FLAGS: &[&str] = &["-es", "-u", "NONE", "-i", "NONE", "-N", "-S"];

/// Neovim's. `--clean` is "factory defaults": no user config, no plugins, no
/// shada. `-i NONE` is redundant with it today and stated anyway, so a change in
/// what `--clean` covers cannot quietly let the developer's shada file in.
/// Neovim has no compatible mode, which is why vimlrs targets vim's `-N` state.
const NVIM_FLAGS: &[&str] = &["--headless", "--clean", "-i", "NONE", "-S"];

/// First executable named `prog` on `PATH`, with symlinks resolved.
fn which(prog: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(prog))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
}

/// The environment both engines run in, pinned — the same set
/// `scripts/parity.sh`'s `pinned()` gives them, and for the same reasons.
///
/// `LC_CTYPE` decides `&encoding`: a non-UTF-8 locale gives `latin1` and every
/// byte-level answer in the corpus moves with it. `LC_MESSAGES` decides whether
/// vim's diagnostics are translated — the E-number survives translation, but
/// `execute()` output in `--stmts` mode does not. `LANGUAGE` is *cleared*, not
/// set: gettext reads it before `LC_ALL` and it is a colon-list, not a locale.
/// `VIM`/`VIMRUNTIME` are removed because a developer who exports `VIM` (a
/// common habit) hands the editor a path that is not a runtime directory, and
/// it then stops finding `$VIMRUNTIME/lang` — which looks exactly like a
/// correctly pinned locale while being a broken reference editor.
fn pin_env(cmd: &mut Command) {
    for k in ["LC_CTYPE", "LC_COLLATE", "LC_TIME", "VIM", "VIMRUNTIME"] {
        cmd.env_remove(k);
    }
    cmd.env("LC_ALL", "C.UTF-8")
        .env("LANG", "C.UTF-8")
        .env("LC_MESSAGES", "C.UTF-8")
        .env("LANGUAGE", "")
        .env("TZ", "UTC");
}

impl Oracle {
    /// Resolve one engine, or `None` when it is not installed.
    ///
    /// `$FUZZ_VIM` / `$FUZZ_NVIM` override the PATH lookup so a specific build
    /// can be pinned without reordering PATH; the override is reported like any
    /// other resolution, so it can never be mistaken for the ambient editor.
    fn resolve(name: &'static str) -> Option<Oracle> {
        let env_key = if name == "vim" {
            "FUZZ_VIM"
        } else {
            "FUZZ_NVIM"
        };
        let bin = match std::env::var_os(env_key) {
            Some(p) => std::fs::canonicalize(p).ok()?,
            None => which(name)?,
        };
        let mut cmd = Command::new(&bin);
        pin_env(&mut cmd);
        let out = cmd.arg("--version").output().ok()?;
        let version = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or("?")
            .to_string();
        Some(Oracle {
            name,
            bin,
            version,
            flags: if name == "vim" { VIM_FLAGS } else { NVIM_FLAGS },
            want_compatible: if name == "vim" { Some(0) } else { None },
        })
    }

    /// A `Command` for this oracle: the pinned binary, the pinned environment,
    /// the pinned flag vector, and `script` last (every vector ends in `-S`).
    fn command(&self, script: &Path) -> Command {
        let mut cmd = Command::new(&self.bin);
        pin_env(&mut cmd);
        cmd.args(self.flags).arg(script);
        cmd
    }

    /// The one-line self-report. Printed before any corpus runs, because a
    /// finding is only reproducible if the reader can relaunch the exact editor
    /// that produced it.
    fn describe(&self, script_placeholder: &str) -> String {
        format!(
            "  {:<5} {}\n        {}\n        argv: {} {} {}",
            self.name,
            self.bin.display(),
            self.version,
            self.bin.display(),
            self.flags.join(" "),
            script_placeholder
        )
    }

    /// Prove the flag vector took, before a single case is judged against it.
    ///
    /// Three things are checked, and each one has silently broken a run before:
    ///
    /// * `&encoding` is `utf-8` — a libc without `C.UTF-8` falls back to `C`
    ///   *silently*, and every byte-level answer would then be latin1.
    /// * `&compatible` is 0 for vim — the 14-observable split above.
    /// * `~/.viminfo` is not written. This is the flag vector's most damaging
    ///   failure mode because it is invisible in the output: dropping `-i NONE`
    ///   makes the reference editor persist registers and histories to the
    ///   developer's disk, so run N+1 answers differently from run N and neither
    ///   is the editor's factory behaviour.
    fn preflight(&self, tmp: &Path) -> Result<(), String> {
        let script = tmp.join(format!("preflight_{}.vim", self.name));
        let out = tmp.join(format!("preflight_{}.txt", self.name));
        let _ = std::fs::remove_file(&out);
        let body = format!(
            "call writefile([&encoding, string(&compatible), string(&loadplugins)], {})\nqa!\n",
            vim_quote(&out.display().to_string())
        );
        std::fs::write(&script, body).map_err(|e| e.to_string())?;

        let viminfo = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".viminfo"));
        let before = viminfo.as_ref().and_then(|p| p.metadata().ok()).map(|m| {
            (
                m.len(),
                m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            )
        });

        let mut child = self
            .command(&script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{} would not start: {e}", self.name))?;
        if !wait_bounded(&mut child, CHUNK_TIMEOUT) {
            return Err(format!("{} hung on the preflight probe", self.name));
        }
        let text = std::fs::read_to_string(&out)
            .map_err(|_| format!("{} wrote no preflight answer", self.name))?;
        let got: Vec<&str> = text.lines().collect();
        let enc = got.first().copied().unwrap_or("");
        if enc != "utf-8" {
            return Err(format!(
                "{} came up with encoding={enc}, not utf-8 — this C library has no \
                 C.UTF-8 locale and every byte-level answer would be recorded in latin1",
                self.name
            ));
        }
        if let Some(want) = self.want_compatible {
            let compat: i64 = got.get(1).and_then(|s| s.parse().ok()).unwrap_or(-1);
            if compat != want {
                return Err(format!(
                    "{} came up with compatible={compat}, not {want} — the flag vector \
                     `{}` did not take, and 14 observables differ between the two states",
                    self.name,
                    self.flags.join(" ")
                ));
            }
        }
        let after = viminfo.as_ref().and_then(|p| p.metadata().ok()).map(|m| {
            (
                m.len(),
                m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            )
        });
        if before != after {
            return Err(format!(
                "{} WROTE ~/.viminfo during the preflight probe — `-i NONE` is not in \
                 effect, so this run would read the developer's registers and histories \
                 and leave its own behind",
                self.name
            ));
        }
        Ok(())
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
    /// A funcref — `function('strlen')` or a lambda — for `call`/`funcref`.
    Func,
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
    ("tr", &[Str, Str, Str]),
    ("slice", &[Any, Num, Num]),
    ("sha256", &[Str]),
    ("charclass", &[Str]),
    ("strutf16len", &[Str, Bool]),
    ("utf16idx", &[Str, Num, Bool, Bool]),
    ("byteidxcomp", &[Str, Num]),
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
    ("extendnew", &[List, List, Num]),
    ("mapnew", &[Any, Lambda]),
    ("call", &[Func, List]),
    ("funcref", &[Str, List]),
    ("matchfuzzypos", &[StrList, Str]),
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
    ("js_encode", &[Any]),
    ("js_decode", &[Str]),
    ("msgpackdump", &[List]),
    // `id()` is NOT here, deliberately: it returns the address of the value's
    // heap object (`'0x9ab06df80'` in nvim, `'000c4ce17'` in vim, a third
    // pointer here), so its result differs on every run of every engine and can
    // never be compared. It was on the list, and it produced 4-5 permanent false
    // findings per seed — exactly the "false gap on every run" this list's rule
    // above forbids. Nothing is lost: no outcome it can produce carries signal.
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
    // Double-quoted escapes — a lexer/eval_string surface a single-quoted pool
    // can never reach (`\<Esc>` is what R5-13 was).
    "\"\\<Esc>\"",
    "\"\\<C-A>\"",
    "\"\\<Space>x\"",
    "\"\\x41\\x42\"",
    "\"\\101\\102\"",
    "\"\\u00e9\"",
    "\"a\\\\b\"",
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

/// Funcref values: a named builtin, a lambda, and a partial.
const FUNCS_REF: &[&str] = &[
    "function('strlen')",
    "function('toupper')",
    "function('abs')",
    "function('add')",
    "{x -> x}",
    "{x -> type(x)}",
    "function('printf', ['%d'])",
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
        Func => rng.pick(FUNCS_REF).to_string(),
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

// ─── Statement generation ───────────────────────────────────────────────────
//
// The generator above only ever produced *expressions*, and the two worst bugs
// this harness's targets ever had — `:try` not catching a runtime error, and a
// failed `:let` storing a corrupted value — live at the **statement** level and
// had to be found by hand.
//
// A statement snippet is compared through the very same three-engine pipeline by
// wrapping it in `execute()`, which runs the commands and returns their output as
// a string. So `let x = 1 | echo x` becomes the expression
// `execute("let x = 1 | echo x")`, whose value is "\n1" in every engine — and any
// divergence in control flow, error handling, or output shows up as a plain value
// mismatch.

// ─── Regex generation ───────────────────────────────────────────────────────
//
// `viml_regex` is the largest hand-written carve-out in the crate: it reproduces
// Vim's pattern *dialect* from the documentation rather than porting
// `regexp_bt.c`/`regexp_nfa.c`. That makes it the most likely place for a
// divergence to hide, and drawing patterns from a fixed list (as `PATS` does) only
// ever exercises the shapes someone already thought of.
//
// So build patterns from a grammar instead, and check each one through every API
// that reaches the engine — `match()` (the index), `matchstr()` (the text),
// `matchlist()` (the capture groups), `substitute()` (the rewrite, with the
// zero-width and `\zs` rules) and `split()` — against a subject pool chosen to
// straddle the boundaries: empty, ASCII, multibyte, combining, punctuation, digits.

/// Subjects a generated pattern is matched against.
const RX_SUBJECTS: &[&str] = &[
    "''",
    "'a'",
    "'abc'",
    "'aaa'",
    "'abcabc'",
    "'a1b2c3'",
    "'Hello World'",
    "'  spaced  '",
    "'a.b*c'",
    "'foo_bar-baz'",
    "'ünïcø∂é'",
    "'日本語abc'",
    "'e\u{0301}combining'",
    "'tab\there'",
    "'AbCdEf'",
    // Subjects made of the pattern language's own metacharacters. Without these
    // a "literal `*`" rule (`:help /star`) can only ever be exercised by
    // accident: every subject above answers -1 for the correct reading AND for
    // the wrong one, so the bug is invisible. `^*` was found this way.
    "'*'",
    "'**'",
    "'*a'",
    "'a*b'",
    "'^caret'",
    "'dollar$'",
    "'a\\b'",
    "'[](){}'",
    "'~tilde~'",
    "'a|b'",
    "'\\zs'",
    // Long enough that a quantifier's backtracking is observable, and a run
    // where an off-by-one in a `\{n,m}` bound shows up as a different answer
    // rather than the same one.
    "repeat('ab', 12)",
    "repeat('a', 30)",
    "'aaab'",
    "'   '",
    "'0x1f'",
];

/// A regex *atom* — the leaves of the grammar.
fn rx_atom(rng: &mut Rng) -> String {
    match rng.below(18) {
        0 => rng
            .pick(&["a", "b", "c", "x", "1", "_", "-", ".", "é"])
            .to_string(),
        1 => ".".into(),
        2 => rng
            .pick(&[
                "\\d", "\\D", "\\w", "\\W", "\\s", "\\S", "\\a", "\\l", "\\u", "\\x", "\\o", "\\h",
                "\\k", "\\p", "\\i",
            ])
            .to_string(),
        3 => rng
            .pick(&[
                "[abc]", "[^abc]", "[a-z]", "[A-Z0-9]", "[^0-9]", "[.*]", "[]a]",
            ])
            .to_string(),
        4 => rng
            .pick(&[
                "[[:alpha:]]",
                "[[:digit:]]",
                "[[:space:]]",
                "[[:punct:]]",
                "[[:upper:]]",
                "[[:alnum:]]",
            ])
            .to_string(),
        5 => format!("\\({}\\)", rx_branch(rng, 0)),
        6 => format!("\\%({}\\)", rx_branch(rng, 0)),
        7 => rng.pick(&["\\<", "\\>", "^", "$"]).to_string(),
        8 => rng.pick(&["\\zs", "\\ze"]).to_string(),
        9 => rng
            .pick(&["\\%d97", "\\%x62", "\\%o143", "\\%u0061"])
            .to_string(),
        10 => rng.pick(&["\\_.", "\\_s", "\\_a", "\\_[a-z]"]).to_string(),
        11 => format!("\\%[{}]", rng.pick(&["abc", "ab", "xyz"])),
        12 => rng.pick(&["\\1", "\\2"]).to_string(),
        // Collections whose *edges* are the interesting part: a `]` first, a
        // trailing/leading `-`, a backslash class inside, a negated `]`.
        13 => rng
            .pick(&[
                "[]-]",
                "[^]]",
                "[a-]",
                "[-a]",
                "[\\d]",
                "[\\]]",
                "[[=a=]]",
                "[\\x41-\\x43]",
            ])
            .to_string(),
        // Buffer-position atoms. They parse everywhere and only ever match at
        // the ends of a one-line subject, which is exactly why they belong here:
        // an engine that silently drops an unknown `\%…` atom answers the same
        // as one that honours it, until the atom is the only thing in the branch.
        14 => rng
            .pick(&["\\%^", "\\%$", "\\%V", "\\%1l", "\\%>1c", "\\%<3c"])
            .to_string(),
        // The magic `~` (last substitute string) and its nomagic spelling.
        15 => rng.pick(&["~", "\\~"]).to_string(),
        // A nested optional sequence, which the flat `\%[abc]` above never
        // produces.
        16 => format!(
            "\\%[{}\\({}\\)]",
            rng.pick(&["a", "x"]),
            rng.pick(&["b", "y"])
        ),
        _ => rng
            .pick(&["\\.", "\\*", "\\[", "\\\\", "\\/", "\\$", "\\^"])
            .to_string(),
    }
}

/// An atom plus an optional quantifier.
fn rx_piece(rng: &mut Rng, depth: u32) -> String {
    let atom = if depth > 0 && rng.chance(1, 5) {
        format!("\\({}\\)", rx_branch(rng, depth - 1))
    } else {
        rx_atom(rng)
    };
    match rng.below(14) {
        0 => format!("{atom}*"),
        1 => format!("{atom}\\+"),
        2 => format!("{atom}\\?"),
        3 => format!("{atom}\\="),
        4 => format!("{atom}\\{{2}}"),
        5 => format!("{atom}\\{{1,3}}"),
        6 => format!("{atom}\\{{-}}"),
        7 => format!("{atom}\\{{-1,}}"),
        // The bounds Vim lets you omit on either side, and the empty one.
        8 => format!("{atom}\\{{,3}}"),
        9 => format!("{atom}\\{{2,}}"),
        10 => format!("{atom}\\{{}}"),
        // LOOKAROUND — `\@=`, `\@!`, `\@<=`, `\@<!`, `\@>`. The whole family
        // was missing from this grammar while `viml_regex` carries a hand-written
        // implementation of it (`Node::Look`, `LookOp`), including the bounded
        // `\@123<=` form whose limit changes how far back the engine may start.
        11 | 12 => {
            let atom = if atom.starts_with("\\(") {
                atom
            } else {
                format!("\\({atom}\\)")
            };
            format!(
                "{atom}{}",
                rng.pick(&["\\@=", "\\@!", "\\@<=", "\\@<!", "\\@>", "\\@3<=", "\\@2<!"])
            )
        }
        _ => atom,
    }
}

/// A concatenation, possibly with alternation.
fn rx_branch(rng: &mut Rng, depth: u32) -> String {
    let n = 1 + rng.below(3);
    let mut out = String::new();
    for _ in 0..n {
        out.push_str(&rx_piece(rng, depth));
    }
    if rng.chance(1, 4) {
        out.push_str("\\|");
        out.push_str(&rx_piece(rng, depth));
    }
    // `\&` — all concats must match at the same position and the LAST one is
    // what gets matched. `viml_regex` compiles the leading ones to zero-width
    // lookaheads (`and_branch`), a rewrite nothing in this grammar reached.
    if rng.chance(1, 12) {
        out.push_str("\\&");
        out.push_str(&rx_piece(rng, depth));
    }
    out
}

/// One generated pattern, wrapped as a single-quoted VimL string.
fn rx_pattern(rng: &mut Rng) -> String {
    let body = rx_branch(rng, 1);
    // Occasionally force case control or a magic-mode switch — both change how the
    // whole pattern is read.
    let prefix = match rng.below(8) {
        0 => "\\c",
        1 => "\\C",
        2 => "\\v",
        3 => "\\V",
        4 => "\\M",
        _ => "",
    };
    // A magic switch applies from where it appears to the end of the pattern, so
    // one in the MIDDLE is a different code path from one at the front: the
    // translation into the parser's dialect has to change modes mid-string and
    // keep the two halves' escaping straight.
    if rng.chance(1, 6) {
        let tail = rx_branch(rng, 0);
        let mid = rng.pick(&["\\v", "\\V", "\\M", "\\m", "\\c", "\\C"]);
        return format!("'{prefix}{body}{mid}{tail}'");
    }
    format!("'{prefix}{body}'")
}

/// One regex case: a generated pattern, a subject, and one of the APIs that reach
/// the engine. Comparing several APIs matters — a pattern can match in the same
/// place yet report different capture groups or rewrite differently.
fn gen_regex(rng: &mut Rng) -> String {
    let pat = rx_pattern(rng);
    let subj = rng.pick(RX_SUBJECTS);
    match rng.below(14) {
        0 => format!("match({subj}, {pat})"),
        1 => format!("matchstr({subj}, {pat})"),
        2 => format!("matchend({subj}, {pat})"),
        3 => format!("matchlist({subj}, {pat})"),
        4 => format!("substitute({subj}, {pat}, 'X', '')"),
        5 => format!("substitute({subj}, {pat}, 'X', 'g')"),
        6 => format!("substitute({subj}, {pat}, '[&]', 'g')"),
        7 => format!("split({subj}, {pat})"),
        // The start/count arguments: `match(s, p, start, count)` restarts the
        // search and picks the Nth hit, which is a different traversal from the
        // first-hit path every case above takes.
        8 => format!(
            "match({subj}, {pat}, {})",
            rng.pick(&["0", "1", "2", "-1", "-3"])
        ),
        9 => format!("match({subj}, {pat}, 0, {})", rng.pick(&["1", "2", "3"])),
        // Byte offsets alongside the text — a different accessor over the same
        // match, and the one place a char-vs-byte index error shows up.
        10 => format!("matchstrpos({subj}, {pat})"),
        // `\=` in the replacement evaluates an expression per match, with
        // `submatch()` for the groups.
        11 => format!("substitute({subj}, {pat}, '\\=submatch(0)', 'g')"),
        // `~` in a replacement is the PREVIOUS replacement, and `\U`/`\l` are
        // the case-folding escapes.
        12 => format!("substitute({subj}, {pat}, {}, 'g')", rng.pick(REPLS)),
        // `keepempty` changes whether a zero-width match yields empty fields.
        _ => format!("split({subj}, {pat}, {})", rng.pick(&["0", "1"])),
    }
}

/// Statement shapes beyond a bare `:echo`/`:let` — the surfaces the generator was
/// blind to: user functions (arguments, defaults, varargs, closures, recursion),
/// compound and unpacking `:let`, indexed assignment, `:unlet`, `:for` over a Dict
/// or String, and string interpolation. Each is a complete snippet; `%E` is
/// substituted with a generated expression.
const STMT_SHAPES: &[&str] = &[
    // user functions: argument scope, defaults, varargs, return
    "function! F(a) \n return a:a \n endfunction \n echo F(%E)",
    "function! F(a, b = 2) \n return a:b \n endfunction \n echo F(1)",
    "function! F(...) \n return a:0 \n endfunction \n echo F(%E, 2)",
    "function! F(...) \n return a:000 \n endfunction \n echo F(%E)",
    "function! F(a) \n if a:a > 0 \n return 'p' \n endif \n return 'n' \n endfunction \n echo F(1)",
    // a closure over a local
    "function! F() closure \n return 1 \n endfunction \n echo F()",
    "let l:x = %E | let Fn = {-> l:x} | echo Fn()",
    // compound assignment
    "let x = 1 | let x += %E | echo x",
    "let s = 'a' | let s .= 'b' | echo s",
    "let n = 10 | let n -= 3 | echo n",
    // unpacking
    "let [a, b] = [1, 2] | echo a + b",
    "let [a; rest] = [1, 2, 3] | echo rest",
    // indexed / member assignment
    "let l = [1,2,3] | let l[0] = %E | echo l",
    "let d = {'a': 1} | let d.a = %E | echo d",
    "let d = {'a': 1} | let d['b'] = 2 | echo d",
    "let l = [1,2,3] | let l[0:1] = [9,9] | echo l",
    // unlet
    "let g:tmp = 1 | unlet g:tmp | echo exists('g:tmp')",
    "let d = {'a':1,'b':2} | unlet d.a | echo d",
    // :for over the containers
    "for [k, v] in items({'a':1}) | echo k . v | endfor",
    "for c in split('ab', '\\zs') | echo c | endfor",
    "for b in 0z0102 | echo b | endfor",
    "for i in range(3) | if i == 1 | continue | endif | echo i | endfor",
    "for i in range(5) | if i == 2 | break | endif | echo i | endfor",
    // string interpolation
    "let x = %E | echo $'v={x}'",
    // while
    "let i = 0 | while i < 3 | let i += 1 | endwhile | echo i",
    // ── surfaces the table was blind to ───────────────────────────────────
    // `:const` and the lock family. A `:const` reassignment is an ERROR, and
    // "which statement in the line still ran" is exactly what the `|`-line
    // model gets wrong when it gets it wrong.
    "const c = %E | echo c",
    "const c = 1 | try | let c = 2 | catch | echo 'E:' . v:exception[:14] | endtry | echo c",
    "let l = [1,2] | lockvar l | try | call add(l, 3) | catch | echo 'locked' | endtry | echo l",
    "let l = [1,2] | lockvar l | unlockvar l | call add(l, 3) | echo l",
    // `:for` over a locked container, and modifying the list being iterated.
    "let l = [1,2,3] | for x in l | if x == 2 | call remove(l, 0) | endif | endfor | echo l",
    // Heredoc assignment, including the trim/eval variants.
    "let t =<< END\nab\ncd\nEND\necho t",
    "let t =<< trim END\n  ab\n  cd\n  END\necho t",
    // Exception flow the shape table never reached: a re-`throw` from a catch,
    // `:finally` running on both paths, a pattern-matched `:catch`, and the
    // value `v:exception`/`v:throwpoint` carry.
    "try | throw 'boom' | catch /^boom$/ | echo 'matched' | finally | echo 'fin' | endtry",
    "try | try | throw 'inner' | finally | echo 'f1' | endtry | catch | echo 'outer:' . v:exception | endtry",
    "try | echoerr 'bad' | catch | echo v:exception[:6] | endtry",
    "function! F() \n try \n return 1 \n finally \n echo 'fin' \n endtry \n endfunction \n echo F()",
    // `:return` with no value, and a function that falls off the end.
    "function! F() \n return \n endfunction \n echo F()",
    "function! F() \n let x = %E \n endfunction \n echo F()",
    // Recursion and a funcref round-trip.
    "function! F(n) \n return a:n <= 0 ? 0 : a:n + F(a:n - 1) \n endfunction \n echo F(4)",
    "function! F(a) \n return a:a * 2 \n endfunction \n let R = function('F') \n echo R(3)",
    "function! F(a) \n return a:a \n endfunction \n echo call('F', [%E])",
    // A dict function and `self`.
    "let d = {'n': 2} | function! d.tw() dict \n return self.n * 2 \n endfunction \n echo d.tw()",
    // Closures that capture a loop variable — the classic binding question.
    "let fs = [] | for i in range(3) | let fs += [{-> i}] | endfor | echo map(copy(fs), 'v:val()')",
    // `:elseif` chains and the empty-block forms.
    "let x = %E | if x is 0 | echo 'a' | elseif x is 1 | echo 'b' | else | echo 'c' | endif",
    // Nested `execute`, and `:silent!` swallowing an error mid-line.
    "execute 'echo ' . string(%E)",
    "silent! call nosuchfunction() | echo 'after'",
    // `:let` with a nested target and a slice on the right.
    "let d = {'a': [1,2,3]} | let d.a[1] = %E | echo d",
    "let l = [1,2,3,4] | echo l[1:2] | let l[1:2] = [8,9] | echo l",
    // Compound assignment on containers.
    "let l = [1] | let l += [2] | echo l",
    "let d = {'a':1} | let d2 = extend(copy(d), {'b':2}) | echo sort(keys(d2))",
];

/// One VimL *command*, for use inside a statement snippet.
fn gen_cmd(rng: &mut Rng, funcs: &[(&str, &[Shape])], depth: u32) -> String {
    let e = |rng: &mut Rng| gen_expr(rng, funcs, depth);
    match rng.below(12) {
        0 => format!("echo {}", e(rng)),
        1 => format!("let x = {}", e(rng)),
        2 => format!("let g:l = {}", e(rng)),
        3 => format!("echo {} | echo {}", e(rng), e(rng)),
        // The `|`-separated line: Vim abandons the rest of it when one errors.
        4 => format!("echo {} | echo 'tail'", e(rng)),
        5 => format!("if {} | echo 'y' | else | echo 'n' | endif", e(rng)),
        6 => format!("for i in {} | echo i | endfor", rng.pick(LISTS)),
        7 => format!("try | echo {} | catch | echo 'caught' | endtry", e(rng)),
        8 => format!("try\necho {}\ncatch\necho 'caught'\nendtry", e(rng)),
        9 => format!("silent! echo {}", e(rng)),
        10 => format!("let d = {} | echo d", e(rng)),
        _ => format!("call add(g:l, {}) | echo g:l", e(rng)),
    }
}

/// A statement snippet, wrapped as `execute('…')` so it rides the expression
/// pipeline (see the note above). Newlines inside the snippet become `\n` in the
/// single-quoted VimL string, which `execute()` treats as separate command lines —
/// that is how the multi-line `:try` form is reached.
fn gen_stmt(rng: &mut Rng, funcs: &[(&str, &[Shape])], depth: u32) -> String {
    // Half the snippets come from the shape table (user functions, `:let` targets,
    // `:for` forms …), half from the free-form command generator.
    let cmd = if rng.chance(1, 2) {
        rng.pick(STMT_SHAPES)
            .replace("%E", &gen_expr(rng, funcs, depth.min(1)))
    } else {
        gen_cmd(rng, funcs, depth)
    };
    // `execute()` takes the command text; a literal newline cannot appear inside a
    // single-quoted VimL string, so the generator emits the two-character escape
    // `\n` and `execute()` splits on it.
    format!("execute({})", vim_quote(&cmd))
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
        // OBSERVE, don't capture. `capture_errors_begin` is Vim's `emsg_silent`
        // path, and a silenced error is deliberately never turned into an exception
        // (`cause_errthrow` declines) — so capturing in order to *read* the error
        // silently disabled `:try`/`:catch` in everything the fuzzer ran, and it
        // duly reported "vimlrs does not catch runtime errors" for a dozen cases the
        // real binary catches fine. The observer reads the message and changes
        // nothing.
        observe_errors_begin();
        for line in PRELUDE {
            let _ = vimlrs::eval_source(line);
        }
        let _ = observe_errors_take();

        // `did_emsg` is the "an error was reported and NOT handled" flag: `:catch`
        // resets it (c: ex_catch), and `:silent!` never sets it. So it — not the
        // presence of an observed message — is what decides whether this expression
        // ends in an error, exactly as it decides a script's exit status.
        did_emsg.with(|d| d.set(0));
        observe_errors_begin();
        let out = vimlrs::eval_expr(expr);
        let errs = observe_errors_take();
        let unhandled = did_emsg.with(|d| d.get()) != 0;
        match out {
            Ok(v) if !unhandled => Outcome::Val(encode_tv2string(&v).to_string()),
            // The FIRST unhandled message: Vim reports an error and abandons the
            // command, so the first one is what it shows. This VM keeps evaluating
            // and can raise more, and a `:catch` clears the ones it handled — so
            // what is left here begins with the error Vim would have reported.
            Ok(_) => Outcome::Err(enumber(errs.first().map_or("E?", |s| s.as_str()))),
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
///
/// The editor is [`Oracle`], not a PATH name: one absolute binary, one flag
/// vector, one pinned environment, preflighted before it is trusted.
fn run_oracle(o: &Oracle, exprs: &[String], tmp: &Path) -> Vec<Outcome> {
    run_oracle_until(o, exprs, tmp, None)
}

/// [`run_oracle`], but abandoning the corpus once `deadline` passes; whatever is
/// left stays [`Outcome::Missing`].
///
/// The shrinker needs this and the main run must not have it. A pathological
/// candidate costs a whole [`CHUNK_TIMEOUT`] and the loop then RESTARTS after it,
/// so a batch holding a handful of them costs minutes — and the shrinker builds
/// batches out of deliberately mangled patterns, which is exactly where the
/// editor is most likely to wedge. Unbounded, shrinking 46 findings did not
/// finish in an hour. A missing answer only means a candidate is not accepted as
/// a reduction, never that a finding is lost.
fn run_oracle_until(
    o: &Oracle,
    exprs: &[String],
    tmp: &Path,
    deadline: Option<Instant>,
) -> Vec<Outcome> {
    let script = tmp.join(format!("drv_{}.vim", o.name));
    let out = tmp.join(format!("out_{}.txt", o.name));
    let _ = std::fs::remove_file(&out);

    let mut res = vec![Outcome::Missing; exprs.len()];
    let mut next = 0usize;

    while next < exprs.len() {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        let hi = (next + CHUNK).min(exprs.len());
        std::fs::write(&script, driver(exprs, next, hi, &out)).expect("write driver");

        let spawned = o
            .command(&script)
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

/// One distinct finding, keyed in the report by its signature.
///
/// The rendered strings are what the report prints; the raw [`Outcome`]s are what
/// the shrinker compares against, since telling "the same finding, smaller" from
/// "also broken, differently" needs the E-numbers and value/error shapes rather
/// than their display form.
struct Finding {
    hits: usize,
    expr: String,
    shown_mine: String,
    shown_oracles: String,
    mine: Outcome,
    nv: Outcome,
    vi: Outcome,
}

// ─── Triage ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Ok,
    Gap,
    Panic,
    /// The oracles disagree AND vimlrs matches neither of them.
    ///
    /// Actionable, not advisory. Two references that disagree still bracket the
    /// answer between them; a third result outside that bracket is vimlrs's own.
    Neither,
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
        // engine, so matching nvim is parity, and matching vim alone is the
        // advisory split. Matching NEITHER is a finding: there is no single spec
        // to be wrong about, but there is no reading of either spec that
        // produces a third answer.
        _ => {
            if v == nv {
                Class::Ok
            } else if v == vi {
                Class::Divergent
            } else {
                Class::Neither
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

// ─── Shrinking ──────────────────────────────────────────────────────────────
//
// A grammar that nests produces findings nobody can read. This is a real one,
// verbatim, from `--regex --seed 11`:
//
//   matchlist('a1b2c3', '\(\(\\\{-}\_[a-z]\?.\{-}\)\{1,3}\)\{1,3}\(\l\{-}\_[a-z]\{-1,}.\)\+\(^\{2}\(\**\)\+_\{-1,}\|\[\{-1,}\)\?')
//
// Every one of those constructs is a suspect and only one of them is the bug, so
// the first hour of every finding used to be spent deleting pieces by hand and
// re-running the oracle. That is a mechanical search, and the harness already
// owns both engines — so it does the search.
//
// The reduction is greedy delta-debugging over the case TEXT, which is the one
// representation every mode shares (expressions, `execute(...)` statements and
// regex cases are all just strings by the time they reach the oracle):
//
//   * delete a window of characters, at halving widths — the coarse cut that
//     removes whole alternations and argument lists in one step;
//   * unwrap one `\(…\)` / `\%(…\)` group, keeping its body;
//   * drop one `\|branch`;
//   * drop one quantifier (`*`, `\+`, `\?`, `\=`, `\{…}`).
//
// A candidate is KEPT only when it still reproduces — when `classify` still
// calls it `Gap`, `Panic` or `Neither`. That predicate is the whole safety
// property: a reduction cannot turn a finding into a passing case and call it
// progress, and it cannot invent a finding either, because the same three-engine
// triage judges the reduced case as judged the original.
//
// Each round evaluates EVERY candidate in one batch through each engine — the
// oracle driver already takes a list, so a round costs three process spawns, not
// three per candidate. That is what makes shrinking affordable enough to leave
// on by default.

/// Candidate reductions of `s`, shortest first. Every one is strictly shorter.
fn shrink_candidates(s: &str) -> Vec<String> {
    let b: Vec<char> = s.chars().collect();
    let n = b.len();
    let mut out: Vec<String> = Vec::new();
    let take = |v: &[char]| -> String { v.iter().collect() };

    // Window deletions at halving widths: the coarse cuts first.
    let mut w = n / 2;
    while w >= 1 {
        let mut i = 0;
        while i < n {
            let hi = (i + w).min(n);
            let mut cand = take(&b[..i]);
            cand.push_str(&take(&b[hi..]));
            if cand.len() < s.len() {
                out.push(cand);
            }
            i += w;
        }
        if w == 1 {
            break;
        }
        w /= 2;
    }

    // Structural reductions, expressed on the byte string.
    for open in ["\\(", "\\%("] {
        let mut from = 0;
        while let Some(i) = s[from..].find(open).map(|k| k + from) {
            if let Some(j) = s[i..].find("\\)").map(|k| k + i) {
                let inner = &s[i + open.len()..j];
                out.push(format!("{}{}{}", &s[..i], inner, &s[j + 2..]));
            }
            from = i + open.len();
        }
    }
    // `a\|b` → `a` and → `b`, one alternation at a time.
    let mut from = 0;
    while let Some(i) = s[from..].find("\\|").map(|k| k + from) {
        out.push(format!("{}{}", &s[..i], &s[i + 2..]));
        from = i + 2;
    }
    // Quantifiers: the suffix, not the atom.
    for q in [
        "\\{-1,}", "\\{1,3}", "\\{-}", "\\{2}", "\\+", "\\?", "\\=", "*",
    ] {
        let mut from = 0;
        while let Some(i) = s[from..].find(q).map(|k| k + from) {
            out.push(format!("{}{}", &s[..i], &s[i + q.len()..]));
            from = i + q.len();
        }
    }

    out.retain(|c| c.len() < s.len() && !c.is_empty() && !c.contains('\n') && !c.contains('\0'));
    out.sort_by_key(|c| (c.len(), c.clone()));
    out.dedup();
    out
}

/// Does `cand` reproduce the SAME finding as `orig`, rather than merely being
/// broken in some other way?
///
/// This predicate is the shrinker's only safety property, and a loose version of
/// it is worse than no shrinking at all. The first cut here kept any candidate
/// `classify` still called actionable, and it reduced eight distinct regex
/// findings to the single characters `,` `(` `)` `:` `-` — every one of which is
/// a *syntax* error in one engine and a different one in the other, i.e. a real
/// but completely unrelated divergence. The reader is then handed a one-character
/// "repro" for a bug about `\%[…]` inside an alternation.
///
/// So the outcome SHAPE has to survive the reduction, engine by engine:
///
/// * an error stays the same error — the same E-number in vimlrs and the same
///   E-number in each oracle (the numbers are the finding; the prose is not);
/// * a value stays a value, in every engine, and vimlrs must still disagree
///   with the oracle (the values themselves change as the case shrinks — that
///   is the point of shrinking);
/// * a panic stays a panic.
fn same_finding(
    orig: (&Outcome, &Outcome, &Outcome),
    cand: (&Outcome, &Outcome, &Outcome),
) -> bool {
    fn shape(a: &Outcome, b: &Outcome) -> bool {
        match (a, b) {
            // Same error means the same E-number, not merely "both errored".
            (Outcome::Err(x), Outcome::Err(y)) => x == y,
            (Outcome::Val(_), Outcome::Val(_)) => true,
            (Outcome::Panic(_), Outcome::Panic(_)) => true,
            (Outcome::Missing, Outcome::Missing) => true,
            _ => false,
        }
    }
    matches!(
        classify(cand.0, cand.1, cand.2),
        Class::Gap | Class::Panic | Class::Neither
    ) && shape(orig.0, cand.0)
        && shape(orig.1, cand.1)
        && shape(orig.2, cand.2)
}

/// Reduce one finding to a smaller case that still reproduces it.
///
/// `budget` caps the candidates evaluated per round; rounds run to a fixpoint or
/// `max_rounds`, whichever comes first, so a pathological case costs bounded
/// time rather than the rest of the run.
///
/// Returns the reduced case together with the three outcomes MEASURED for it —
/// never the original's. Reporting a minimized expression next to the outcomes
/// of the expression it came from would be a fabricated finding: the two agree
/// on the bug but not on the values, and the values are what the reader checks
/// the fix against.
fn shrink(
    expr: &str,
    orig: (&Outcome, &Outcome, &Outcome),
    oracles: &(Option<Oracle>, Option<Oracle>),
    tmp: &Path,
    budget: usize,
    max_rounds: usize,
) -> (String, Outcome, Outcome, Outcome) {
    let mut best = (
        expr.to_string(),
        orig.0.clone(),
        orig.1.clone(),
        orig.2.clone(),
    );
    let deadline = Instant::now() + SHRINK_TIME;
    for _ in 0..max_rounds {
        if Instant::now() >= deadline {
            break;
        }
        let mut cands = shrink_candidates(&best.0);
        cands.truncate(budget);
        if cands.is_empty() {
            break;
        }
        let mine = run_vimlrs(&cands, tmp);
        let nv = match &oracles.0 {
            Some(o) => run_oracle_until(o, &cands, tmp, Some(deadline)),
            None => vec![Outcome::Missing; cands.len()],
        };
        let vi = match &oracles.1 {
            Some(o) => run_oracle_until(o, &cands, tmp, Some(deadline)),
            None => vec![Outcome::Missing; cands.len()],
        };
        // `cands` is sorted shortest-first, so the first hit is the best of the
        // round. The comparison is against the ORIGINAL finding, not against the
        // current best, so a chain of twelve reductions cannot walk the case away
        // from the bug one acceptable step at a time.
        match (0..cands.len()).find(|&i| same_finding(orig, (&mine[i], &nv[i], &vi[i]))) {
            Some(i) => {
                best = (
                    cands[i].clone(),
                    mine[i].clone(),
                    nv[i].clone(),
                    vi[i].clone(),
                )
            }
            None => break,
        }
    }
    best
}

// ─── --dap: whole PROGRAMS through a live debug session ─────────────────────
//
// Everything above generates one *expression* and compares one value. That left
// `viml --dap` — a second compiler (`compile_program_debug`), a marker builtin,
// a pause hook, and a call-depth step machine — with no generated coverage at
// all, and it drifted: the debug compile skipped every `Stmt::Function` and
// discarded `funcs`, so no user function existed under `--dap` and
//
//     function! Add(a, b) … endfunction
//     echo Add(2, 3)
//     echo "done"
//
// printed `done` where vim prints `5` / `done`. A hand-written test caught that
// one, afterwards. The check below is the one that would have caught it first,
// and it needs no oracle to do it: whatever a program prints, it must print the
// same thing under the debugger.
//
// Three findings come out of a session, in descending order of severity:
//
//   | class       | condition                                | meaning                    |
//   |-------------|------------------------------------------|----------------------------|
//   | `DapDrift`  | `--dap` output != plain output           | the debug path diverged    |
//   | `FrameBug`  | a backtrace / step-depth invariant broke | the debugger lied          |
//   | `Gap`       | plain output != vim's                    | ordinary parity gap        |
//
// `DapDrift` and `FrameBug` are self-evidencing — the program is its own
// oracle — so this mode is worth running with no editor installed at all.

/// Stops to allow before a session stops stepping and just runs to the end.
/// A `stepIn`-heavy walk through a nested loop is otherwise unbounded.
const DAP_MAX_STOPS: usize = 150;

/// Per-session ceiling: a debug session that stalls is a finding, not a hang.
const DAP_SESSION_TIMEOUT: Duration = Duration::from_secs(20);

/// A generated program plus the line of its first top-level statement — where
/// the session plants its one breakpoint to get an initial stop to step from.
struct DapProgram {
    src: String,
    first_line: u32,
    lines: u32,
}

/// Build a program whose shape is what a debugger actually traverses: nested
/// user-function calls (so frames stack), branches and loops (so a line is
/// reached more than once), and `echo` at every level (so the output is a
/// fingerprint of the path taken).
///
/// The expression grammar here is deliberately narrow — literals and arithmetic
/// on them, nothing that raises. Expression-level gaps are the other modes' job;
/// mixing them in would bury a debug finding under a hundred known E-numbers.
fn gen_dap_program(rng: &mut Rng) -> DapProgram {
    fn atom(rng: &mut Rng) -> String {
        match rng.below(4) {
            0 => rng.below(50).to_string(),
            1 => format!("'s{}'", rng.below(9)),
            2 => format!("[{}, {}]", rng.below(9), rng.below(9)),
            _ => format!("{} + {}", rng.below(20), rng.below(20)),
        }
    }
    /// An expression over the argument `a:x` of the enclosing function.
    fn over_arg(rng: &mut Rng) -> String {
        match rng.below(4) {
            0 => format!("a:x + {}", rng.below(9)),
            1 => format!("a:x * {}", 1 + rng.below(4)),
            2 => "a:x".to_string(),
            _ => format!("a:x - {}", rng.below(5)),
        }
    }

    let nfuncs = 1 + rng.below(3); // F0 … F2
    let mut src = String::new();
    for i in 0..nfuncs {
        let _ = writeln!(src, "function! F{i}(x)");
        // Every function but the innermost may call the next one down, which is
        // what makes the backtrace more than two frames deep.
        if i + 1 < nfuncs && rng.chance(2, 3) {
            let _ = writeln!(src, "  let d = F{}({})", i + 1, over_arg(rng));
            let _ = writeln!(src, "  echo 'f{i}' . d");
        } else {
            let _ = writeln!(src, "  echo 'f{i}'");
        }
        match rng.below(4) {
            0 => {
                let _ = writeln!(src, "  if a:x > {}", rng.below(10));
                let _ = writeln!(src, "    return {}", atom(rng));
                let _ = writeln!(src, "  endif");
            }
            1 => {
                let _ = writeln!(src, "  let s = 0");
                let _ = writeln!(src, "  for i in range({})", 1 + rng.below(3));
                let _ = writeln!(src, "    let s += i");
                let _ = writeln!(src, "  endfor");
                let _ = writeln!(src, "  echo s");
            }
            2 => {
                let _ = writeln!(src, "  let a = {} | let b = {}", rng.below(9), rng.below(9));
                let _ = writeln!(src, "  echo a + b");
            }
            _ => {}
        }
        let _ = writeln!(src, "  return {}", over_arg(rng));
        let _ = writeln!(src, "endfunction");
    }

    // The first statement that actually runs — the breakpoint anchor.
    let first_line = src.lines().count() as u32 + 1;
    let _ = writeln!(src, "echo 'start'");
    for _ in 0..(1 + rng.below(3)) {
        match rng.below(3) {
            0 => {
                let _ = writeln!(src, "echo F0({})", rng.below(20));
            }
            1 => {
                let _ = writeln!(src, "let r = F0({}) | echo r", rng.below(20));
            }
            _ => {
                let _ = writeln!(src, "echo {}", atom(rng));
            }
        }
    }
    let _ = writeln!(src, "echo 'end'");
    let lines = src.lines().count() as u32;
    DapProgram {
        src,
        first_line,
        lines,
    }
}

/// What one debug session produced.
struct DapSession {
    /// Everything the program printed, via `output` events.
    output: String,
    /// Broken invariants, one message each.
    violations: Vec<String>,
    /// How many times it stopped (0 means the breakpoint never fired).
    stops: usize,
}

/// Run `path` under `viml --dap`, stepping with a seed-driven mix of verbs, and
/// check the backtrace and the step-depth contract at every stop.
fn run_dap_session(bin: &Path, path: &str, prog: &DapProgram, rng: &mut Rng) -> DapSession {
    use serde_json::json;
    use vimlrs::dap_client::Dap;

    let mut v: Vec<String> = Vec::new();
    let mut dap = Dap::spawn_binary(bin);
    let deadline = Instant::now() + DAP_SESSION_TIMEOUT;

    if dap.try_request("initialize", json!({})).is_err() {
        return DapSession {
            output: String::new(),
            violations: vec!["adapter died before initialize".into()],
            stops: 0,
        };
    }
    let _ = dap.try_request(
        "setBreakpoints",
        json!({ "source": { "path": path },
                "breakpoints": [{ "line": prog.first_line }] }),
    );
    let _ = dap.try_request("configurationDone", json!({}));
    let _ = dap.try_request("launch", json!({ "program": path }));

    // `(verb, depth)` of the step that produced the stop we are now examining.
    let mut armed: Option<(&'static str, usize)> = None;
    let mut stops = 0usize;
    // Events are consumed in order: a search that starts from the beginning
    // finds the FIRST `stopped` again on every pass and the loop never advances.
    let mut cursor = 0usize;

    loop {
        let Some(msg) = dap.wait_next(&mut cursor, DAP_SESSION_TIMEOUT, |m| {
            m["type"] == "event" && (m["event"] == "stopped" || m["event"] == "terminated")
        }) else {
            v.push("session stalled: neither `stopped` nor `terminated` arrived".into());
            break;
        };
        if msg["event"] == "terminated" {
            break;
        }
        stops += 1;
        let reason = msg["body"]["reason"].as_str().unwrap_or("").to_string();

        // The backtrace, checked against the program it belongs to. Correlated
        // on the request seq: the session asks at EVERY stop, and a match on the
        // command name alone would keep handing back the first stop's answer.
        let Ok(seq) = dap.try_request("stackTrace", json!({ "threadId": 1 })) else {
            v.push("adapter died on `stackTrace`".into());
            break;
        };
        let Some(st) = dap.try_wait_response_seq(seq, DAP_SESSION_TIMEOUT) else {
            v.push("no stackTrace response while stopped".into());
            break;
        };
        let frames: Vec<(String, i64)> = st["body"]["stackFrames"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|f| {
                        (
                            f["name"].as_str().unwrap_or_default().to_string(),
                            f["line"].as_i64().unwrap_or(-1),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        if frames.is_empty() {
            v.push("stopped with an EMPTY backtrace".into());
            break;
        }
        if st["body"]["totalFrames"].as_i64() != Some(frames.len() as i64) {
            v.push(format!(
                "totalFrames {} != {} frames returned",
                st["body"]["totalFrames"],
                frames.len()
            ));
        }
        // Depth 0 is the script itself, and it is the only frame that can be.
        if frames[frames.len() - 1].0 != "script" {
            v.push(format!(
                "outermost frame is `{}`, not `script`",
                frames[frames.len() - 1].0
            ));
        }
        if frames[..frames.len() - 1]
            .iter()
            .any(|(n, _)| n == "script")
        {
            v.push("an inner frame is named `script`".into());
        }
        for (name, line) in &frames {
            if *line < 1 || *line > prog.lines as i64 {
                v.push(format!(
                    "frame `{name}` on line {line}, outside the program's 1..={}",
                    prog.lines
                ));
            }
        }
        let depth = frames.len() - 1;

        // The step contract, exactly as `should_stop` states it. A breakpoint
        // outranks a step, so a breakpoint stop is not the step's answer and is
        // not judged as one.
        if reason == "step" {
            match armed {
                Some(("next", d)) if depth > d => v.push(format!(
                    "`next` from depth {d} stopped at depth {depth} — it stepped INTO the call"
                )),
                Some(("stepOut", d)) if depth >= d => v.push(format!(
                    "`stepOut` from depth {d} stopped at depth {depth} — it never left the frame"
                )),
                _ => {}
            }
        }

        if stops >= DAP_MAX_STOPS || Instant::now() >= deadline {
            let _ = dap.try_request("continue", json!({ "threadId": 1 }));
            armed = None;
            continue;
        }
        // Bias towards stepIn: it is the verb that builds depth, and depth is
        // what the other two are defined against.
        let verb = match rng.below(8) {
            0..=3 => "stepIn",
            4..=5 => "next",
            6 => "stepOut",
            _ => "continue",
        };
        if dap.try_request(verb, json!({ "threadId": 1 })).is_err() {
            v.push(format!("adapter died on `{verb}`"));
            break;
        }
        armed = if verb == "continue" {
            None
        } else {
            Some((verb, depth))
        };
    }

    let output = dap.output();
    dap.shutdown();
    DapSession {
        output,
        violations: v,
        stops,
    }
}

/// Run a whole program through `viml` (no debugger) and return its stdout.
fn run_program_plain(bin: &Path, path: &str) -> Option<String> {
    let child = Command::new(bin)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // `wait_bounded` needs the pipe drained first or a chatty program deadlocks
    // on a full buffer; these programs print a handful of lines.
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Source a whole program in real `vim` and return what it printed, captured
/// through `:redir` (the form the DAP tests record their reference output with).
fn run_program_vim(vim: &Oracle, path: &str, tmp: &Path) -> Option<String> {
    let out = tmp.join("dap_oracle.txt");
    let _ = std::fs::remove_file(&out);
    // The pinned binary and environment, and the pinned flag vector minus its
    // trailing `-S` — this mode drives the file with `-c` so it can wrap the
    // source in `:redir`. Everything that selects the DIALECT (`-u NONE`,
    // `-i NONE`, `-N`) is the same vector `run_oracle` uses, which is the point:
    // one fuzzer must not report findings against two different editors.
    let mut cmd = Command::new(&vim.bin);
    pin_env(&mut cmd);
    let mut child = cmd
        .args(vim.flags.iter().filter(|f| **f != "-S"))
        .arg("-c")
        .arg(format!("redir! > {}", out.display()))
        .arg("-c")
        .arg(format!("source {path}"))
        .arg("-c")
        .arg("redir END")
        .arg("-c")
        .arg("qa!")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    wait_bounded(&mut child, CHUNK_TIMEOUT);
    std::fs::read_to_string(&out).ok()
}

/// `:echo` output compares by non-empty lines: `:redir` opens with a blank line
/// and the DAP transport batches its chunks differently from a terminal write,
/// so neither leading blanks nor chunk boundaries are part of the contract.
///
/// Error reports are dropped from both sides. `:redir` folds vim's messages into
/// the same stream as its output, while `viml` writes them to stderr — so a
/// comparison that kept them would flag every erroring program as a gap purely
/// on where the two engines send a message. What is left is the PATH the program
/// took, which is the question this mode asks; E-numbers are the expression
/// modes' contract, compared there against both engines. (The generator never
/// echoes a string of this shape, so nothing real is filtered.)
fn out_lines(s: &str) -> Vec<&str> {
    fn is_error_line(l: &str) -> bool {
        l.starts_with("Error detected while processing")
            // vim's `line    2:` locator, between the preamble and the E-number.
            || l.strip_prefix("line")
                .is_some_and(|r| r.trim_start().trim_end_matches(':').parse::<u32>().is_ok())
            || l.strip_prefix('E')
                .is_some_and(|r| r.split_once(':').is_some_and(|(n, _)| {
                    !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())
                }))
    }
    s.lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty() && !is_error_line(l))
        .collect()
}

/// The `--dap` mode. Returns the number of findings.
fn dap_mode(args: &Args) -> usize {
    let bin = vimlrs::dap_client::viml_binary();
    if !bin.exists() {
        eprintln!(
            "fuzz-parity: no `viml` binary at {} — run `cargo build` first",
            bin.display()
        );
        std::process::exit(2);
    }
    let tmp = std::env::temp_dir().join("vimlrs-fuzz-dap");
    std::fs::create_dir_all(&tmp).expect("tmp dir");
    let prog_path = tmp.join("prog.vim");
    let prog_str = prog_path.display().to_string();

    let mut rng = Rng(args.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    // Same pinned editor, same preflight, same self-report as the expression
    // modes — this mode used to build its own `Command::new("vim")` argv.
    let vim = Oracle::resolve("vim").unwrap_or_else(|| {
        eprintln!("fuzz-parity --dap: no `vim` on PATH — this mode needs the reference editor");
        std::process::exit(2);
    });
    if let Err(e) = vim.preflight(&tmp) {
        eprintln!("fuzz-parity: oracle preflight failed: {e}");
        std::process::exit(2);
    }
    eprintln!(
        "fuzz-parity --dap: {} programs, seed {} (binary {})\noracle:\n{}",
        args.count,
        args.seed,
        bin.display(),
        vim.describe("-c 'redir! > OUT' -c 'source PROG' -c 'redir END' -c 'qa!'")
    );

    let mut drift = 0usize;
    let mut framebug = 0usize;
    let mut gap = 0usize;
    let mut oracle_fail = 0usize;
    let mut never_stopped = 0usize;
    let mut ok = 0usize;
    // Deduplicated by the first line of the complaint, like the expression modes.
    let mut buckets: BTreeMap<(u8, String), (usize, String)> = BTreeMap::new();
    let note = |slot: u8, sig: String, prog: &str, buckets: &mut BTreeMap<_, _>| {
        let e = buckets
            .entry((slot, sig))
            .or_insert_with(|| (0usize, prog.to_string()));
        e.0 += 1;
    };

    for i in 0..args.count {
        let prog = gen_dap_program(&mut rng);
        std::fs::write(&prog_path, &prog.src).expect("write program");

        let plain = run_program_plain(&bin, &prog_str).unwrap_or_default();
        let session = run_dap_session(&bin, &prog_str, &prog, &mut rng);

        let mut bad = false;
        if out_lines(&session.output) != out_lines(&plain) {
            drift += 1;
            bad = true;
            note(
                0,
                format!(
                    "--dap printed {:?}, plain run printed {:?}",
                    out_lines(&session.output),
                    out_lines(&plain)
                ),
                &prog.src,
                &mut buckets,
            );
        }
        for msg in &session.violations {
            framebug += 1;
            bad = true;
            note(1, msg.clone(), &prog.src, &mut buckets);
        }
        // A session that never stopped exercised nothing — that is a hole in the
        // generator, not a pass, so it is counted separately and never as `ok`.
        if session.stops == 0 {
            never_stopped += 1;
            bad = true;
            note(
                2,
                format!("breakpoint on line {} never fired", prog.first_line),
                &prog.src,
                &mut buckets,
            );
        }

        match run_program_vim(&vim, &prog_str, &tmp) {
            Some(vo) if !out_lines(&vo).is_empty() => {
                if out_lines(&vo) != out_lines(&plain) {
                    gap += 1;
                    bad = true;
                    note(
                        3,
                        format!(
                            "vim printed {:?}, viml printed {:?}",
                            out_lines(&vo),
                            out_lines(&plain)
                        ),
                        &prog.src,
                        &mut buckets,
                    );
                }
            }
            _ => oracle_fail += 1,
        }
        if !bad {
            ok += 1;
        }
        if args.verbose {
            println!(
                "[{i}] stops={} lines={} {}",
                session.stops,
                prog.lines,
                if bad { "FINDING" } else { "ok" }
            );
        }
    }

    let heading = |slot: u8| match slot {
        0 => "DEBUG DRIFT (`--dap` output != plain output)",
        1 => "DEBUGGER INVARIANT BROKEN (backtrace / step depth)",
        2 => "NEVER STOPPED (generator reached nothing)",
        _ => "PARITY GAPS (vim != viml)",
    };
    let mut last = u8::MAX;
    for ((slot, sig), (n, prog)) in &buckets {
        if *slot != last {
            println!("\n══ {} ══", heading(*slot));
            last = *slot;
        }
        println!("\n  {sig}  ×{n}");
        for l in prog.lines() {
            println!("    | {l}");
        }
    }

    println!(
        "\n── summary (--dap) ──\n  ok:            {ok}\n  DEBUG DRIFT:   {drift}\n  INVARIANTS:    {framebug}\n  NEVER STOPPED: {never_stopped}\n  gaps vs vim:   {gap}\n  oracle-fail:   {oracle_fail}"
    );
    drift + framebug + never_stopped + gap
}

// ─── main ───────────────────────────────────────────────────────────────────

struct Args {
    count: usize,
    seed: u64,
    depth: u32,
    only: Vec<String>,
    corpus: Option<String>,
    verbose: bool,
    /// Generate *statements* (`:let`, `:if`, `:for`, `:try`, `|`-separated lines)
    /// instead of expressions, compared through `execute()`.
    stmts: bool,
    /// Generate *regex* cases: grammar-built patterns through match/matchstr/
    /// matchlist/substitute/split.
    regex: bool,
    /// Generate whole PROGRAMS and run each through a live `viml --dap` debug
    /// session, checking the debug output against the plain run, the backtrace
    /// against the program, and each step verb against the depth it promised.
    dap: bool,
    /// Reduce every distinct finding to a minimal reproducing case before
    /// reporting it (on by default; `--no-shrink` turns it off).
    shrink: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        count: 1000,
        seed: 1,
        depth: 2,
        only: Vec::new(),
        corpus: None,
        verbose: false,
        stmts: false,
        regex: false,
        dap: false,
        shrink: true,
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
            "--stmts" => a.stmts = true,
            "--regex" => a.regex = true,
            "--dap" => a.dap = true,
            "--no-shrink" => a.shrink = false,
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
                     --stmts       fuzz STATEMENTS (:let/:if/:for/:try/`|` lines) via execute()\n\
                     --regex       fuzz REGEX: grammar-built patterns x subjects x APIs\n\
                     --dap         fuzz the DEBUGGER: whole programs through a live\n\
                     \x20              `viml --dap` session (debug output vs plain run,\n\
                     \x20              backtrace shape, step-verb depth contract)\n\
                     --no-shrink   report findings as generated, without reducing them\n\
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

    // The debugger mode drives whole programs, not the expression pipeline.
    if args.dap {
        let findings = dap_mode(&args);
        if findings > 0 {
            std::process::exit(1);
        }
        return;
    }

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
        let e = if args.regex {
            gen_regex(&mut rng)
        } else if args.stmts {
            gen_stmt(&mut rng, &funcs, args.depth)
        } else {
            gen_expr(&mut rng, &funcs, args.depth)
        };
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

    // Resolve, preflight and REPORT both oracles before a single case is judged
    // against them. A finding is only reproducible when the reader can relaunch
    // the exact editor that produced it, so the absolute path, the version line
    // and the full argv are printed — not the word "vim".
    let oracles = (Oracle::resolve("nvim"), Oracle::resolve("vim"));
    eprintln!("fuzz-parity: oracle entry points");
    for o in [oracles.0.as_ref(), oracles.1.as_ref()]
        .into_iter()
        .flatten()
    {
        eprintln!("{}", o.describe("DRIVER.vim"));
        if let Err(e) = o.preflight(&tmp) {
            eprintln!("fuzz-parity: oracle preflight failed: {e}");
            std::process::exit(2);
        }
        eprintln!(
            "        preflight: ok (encoding=utf-8, compatible pinned, ~/.viminfo untouched)"
        );
    }
    if oracles.0.is_none() {
        eprintln!("  nvim  NOT INSTALLED — every case will be judged against vim alone");
    }
    if oracles.1.is_none() {
        eprintln!("  vim   NOT INSTALLED — every case will be judged against nvim alone");
    }

    // vimlrs, in child processes (crash/hang/OOM are findings, not lost runs).
    let mine = run_vimlrs(&exprs, &tmp);

    // Oracles, chunked processes with the same guards.
    let nv = match &oracles.0 {
        Some(o) => run_oracle(o, &exprs, &tmp),
        None => vec![Outcome::Missing; exprs.len()],
    };
    let vi = match &oracles.1 {
        Some(o) => run_oracle(o, &exprs, &tmp),
        None => vec![Outcome::Missing; exprs.len()],
    };
    if nv.iter().all(|o| *o == Outcome::Missing) {
        eprintln!("fuzz-parity: nvim produced no results — is it installed?");
    }
    if vi.iter().all(|o| *o == Outcome::Missing) {
        eprintln!("fuzz-parity: vim produced no results — is it installed?");
    }

    // Triage, deduplicated by signature.
    let mut buckets: BTreeMap<(u8, String), Finding> = BTreeMap::new();
    let mut counts = [0usize; 6];
    let mut corpus_lines: Vec<String> = Vec::new();

    for (i, e) in exprs.iter().enumerate() {
        let class = classify(&mine[i], &nv[i], &vi[i]);
        let slot = match class {
            Class::Ok => 0,
            Class::Gap => 1,
            Class::Panic => 2,
            Class::Neither => 3,
            Class::Divergent => 4,
            Class::OracleFail => 5,
        };
        counts[slot] += 1;
        if args.verbose {
            println!(
                "[{}] {e}\n    viml={} nvim={} vim={}",
                match class {
                    Class::Ok => "ok",
                    Class::Gap => "GAP",
                    Class::Panic => "PANIC",
                    Class::Neither => "NEITHER",
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
        let entry = buckets.entry((slot as u8, sig)).or_insert_with(|| Finding {
            hits: 0,
            expr: e.clone(),
            shown_mine: mine[i].show(),
            shown_oracles: format!("nvim={} vim={}", nv[i].show(), vi[i].show()),
            mine: mine[i].clone(),
            nv: nv[i].clone(),
            vi: vi[i].clone(),
        });
        entry.hits += 1;
    }

    // Reduce each DISTINCT finding to a minimal reproducing case. Only the
    // bucket representatives are shrunk — one per signature, not one per hit —
    // so the cost is proportional to the number of real bugs, not to the corpus.
    if args.shrink && !buckets.is_empty() {
        let n = buckets.len().min(SHRINK_MAX_FINDINGS);
        eprintln!(
            "fuzz-parity: shrinking {n} of {} distinct finding(s), <={}s each…",
            buckets.len(),
            SHRINK_TIME.as_secs()
        );
        for (_, entry) in buckets.iter_mut().take(SHRINK_MAX_FINDINGS) {
            let (expr, mine, nv, vi) = shrink(
                &entry.expr,
                (&entry.mine, &entry.nv, &entry.vi),
                &oracles,
                &tmp,
                SHRINK_BUDGET,
                SHRINK_ROUNDS,
            );
            if expr.len() < entry.expr.len() {
                entry.expr = expr;
                entry.shown_mine = mine.show();
                entry.shown_oracles = format!("nvim={} vim={}", nv.show(), vi.show());
                entry.mine = mine;
                entry.nv = nv;
                entry.vi = vi;
            }
        }
    }

    // Report.
    let heading = |slot: u8| match slot {
        1 => "CONFIRMED GAPS (nvim == vim, vimlrs differs)",
        2 => "PANICS (vimlrs crashed)",
        3 => "MATCHES NEITHER ORACLE (nvim != vim, and vimlrs is a third answer)",
        4 => "VIM/NEOVIM DIVERGENCE (advisory — vimlrs matches vim)",
        _ => "OTHER",
    };
    let mut last = 0u8;
    for ((slot, sig), f) in &buckets {
        if *slot != last {
            println!("\n══ {} ══", heading(*slot));
            last = *slot;
        }
        println!("\n  {sig}  ×{}", f.hits);
        println!("    expr:   {}", f.expr);
        println!("    vimlrs: {}", f.shown_mine);
        println!("    oracle: {}", f.shown_oracles);
    }

    println!(
        "\n── summary ──\n  ok:          {}\n  GAPS:        {} ({} distinct)\n  PANICS:      {}\n  NEITHER:     {} ({} distinct)\n  divergent:   {} (advisory — vimlrs matches vim)\n  oracle-fail: {}",
        counts[0],
        counts[1],
        buckets.keys().filter(|(s, _)| *s == 1).count(),
        counts[2],
        counts[3],
        buckets.keys().filter(|(s, _)| *s == 3).count(),
        counts[4],
        counts[5]
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
    if counts[1] > 0 || counts[2] > 0 || counts[3] > 0 {
        std::process::exit(1);
    }
}
