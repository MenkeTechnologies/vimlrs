//! Seeded, script-level differential fuzzer.
//!
//! `fuzz-parity` (src/bin/fuzz_parity.rs) fuzzes single expressions and
//! statements. This one generates WHOLE scripts — assignments through
//! subscripts and ranges, container mutation, aliasing, `:for`/`:while`,
//! `:try`/`:finally`, `:function`, `:execute` — because the divergences that
//! live between statements (a mutation visible through a second name, a loop
//! variable surviving its loop, an error aborting a function but not the script)
//! cannot be seen one statement at a time.
//!
//! Two tests:
//!
//! * `generated_scripts_run_clean` — CI-safe, needs no editor. Every generated
//!   script must run to completion under `viml` without panicking and inside a
//!   deadline. This catches crashes and hangs, not wrong answers.
//! * `script_fuzz_matches_vim` — the differential run, `#[ignore]`d because it
//!   needs `vim` (and uses `nvim` as a second opinion when present). Run it with
//!   `cargo test --test script_fuzz -- --ignored --nocapture`; widen it with
//!   `VIML_FUZZ_COUNT` and `VIML_FUZZ_SEED`. A script counts as a divergence
//!   only when vim and nvim agree with each other and viml differs, the same
//!   rule `tests/data/fuzz_corpus.txt` was recorded under: this port follows the
//!   vendored Neovim sources, so a place where the two editors differ is a
//!   dialect, not a bug.
//!
//! A divergence is shrunk line by line and printed with its seed. Pin a real one
//! by fixing viml and dropping the shrunk script into `tests/parity_cases/` —
//! never by editing this generator to avoid it.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// xorshift64* — small, deterministic, no dependencies.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

/// The value kinds the generator keeps variables of. A kind is a *preference*:
/// one expression in ten is deliberately the wrong kind, so the error paths get
/// the same coverage as the happy ones.
#[derive(Clone, Copy)]
enum Kind {
    Num,
    Str,
    List,
    Dict,
    Blob,
    Float,
}

const KINDS: [Kind; 6] = [
    Kind::Num,
    Kind::Str,
    Kind::List,
    Kind::Dict,
    Kind::Blob,
    Kind::Float,
];

const VARS_PER_KIND: usize = 3;

fn var_name(kind: Kind, i: usize) -> String {
    let prefix = match kind {
        Kind::Num => "n",
        Kind::Str => "s",
        Kind::List => "l",
        Kind::Dict => "d",
        Kind::Blob => "b",
        Kind::Float => "f",
    };
    format!("{prefix}{i}")
}

struct Gen {
    rng: Rng,
    /// Indentation of the line being emitted.
    indent: usize,
    out: Vec<String>,
    /// Counter for `:while` loop variables and user function names.
    fresh: usize,
    /// Percent of expressions generated with a deliberately wrong kind.
    ///
    /// Kept at 0 by default: an ill-typed operand makes vim stop evaluating the
    /// expression at the first failure, and every later operand and message then
    /// depends on the exact FAIL-propagation order of `eval.c`. A run where each
    /// statement errors in a different place says nothing about the rest of the
    /// language, so type confusion is opt-in (`VIML_FUZZ_NOISE`).
    noise: usize,
}

impl Gen {
    fn line(&mut self, text: String) {
        self.out
            .push(format!("{}{}", "  ".repeat(self.indent), text));
    }

    fn var(&mut self, kind: Kind) -> String {
        let i = self.rng.below(VARS_PER_KIND);
        var_name(kind, i)
    }

    fn any_kind(&mut self) -> Kind {
        KINDS[self.rng.below(KINDS.len())]
    }

    // ── expressions ──────────────────────────────────────────────────────

    /// An expression of `kind` (one time in ten, of a random kind instead).
    fn expr(&mut self, kind: Kind, depth: u32) -> String {
        let kind = if self.rng.chance(self.noise) {
            self.any_kind()
        } else {
            kind
        };
        match kind {
            Kind::Num => self.num(depth),
            Kind::Str => self.string(depth),
            Kind::List => self.list(depth),
            Kind::Dict => self.dict(depth),
            Kind::Blob => self.blob(depth),
            Kind::Float => self.float(depth),
        }
    }

    fn any(&mut self, depth: u32) -> String {
        let kind = self.any_kind();
        self.expr(kind, depth)
    }

    fn num(&mut self, depth: u32) -> String {
        if depth == 0 || self.rng.chance(30) {
            return match self.rng.below(3) {
                0 => self.var(Kind::Num),
                _ => self
                    .rng
                    .pick(&[
                        "0", "1", "-1", "2", "3", "7", "10", "255", "0x1f", "-5", "100",
                    ])
                    .to_string(),
            };
        }
        let d = depth - 1;
        // Every operand is a Number or a String on purpose: no shift (a negative
        // amount is an error), no list element or `max()` of a list that may hold
        // anything, no comparison across kinds. An error in an operand is the
        // evaluation-abort gap (BUGS.md R54-O), not something to chase here.
        match self.rng.below(11) {
            0 => {
                let (a, b) = (self.num(d), self.num(d));
                let op = self.rng.pick(&["+", "-", "*", "/", "%"]);
                format!("({a} {op} {b})")
            }
            1 => match self.rng.below(4) {
                0 => format!("len({})", self.list(d)),
                1 => format!("len({})", self.dict(d)),
                2 => format!("len({})", self.string(d)),
                _ => format!("len({})", self.blob(d)),
            },
            2 => format!("index({}, {})", self.list(d), self.scalar(d)),
            3 => format!("count({}, {})", self.list(d), self.scalar(d)),
            4 => format!("stridx({}, {})", self.string(d), self.string(d)),
            5 => format!("str2nr({})", self.string(d)),
            6 => format!("type({})", self.any(d)),
            7 => format!("empty({})", self.any(d)),
            8 => format!("abs({})", self.num(d)),
            9 => self.comparison(d),
            _ => format!("char2nr({})", self.string(d)),
        }
    }

    /// A Number or a String — what can sit beside its own kind in `==`, `<`,
    /// `index()` or `count()` without an error.
    fn scalar(&mut self, depth: u32) -> String {
        if self.rng.chance(50) {
            self.num(depth)
        } else {
            self.string(depth)
        }
    }

    /// A comparison of two operands of ONE kind, with an operator that kind
    /// accepts (`<` on a List, or a List against a Number, is an error).
    fn comparison(&mut self, depth: u32) -> String {
        let (a, b, op) = match self.rng.below(4) {
            0 => (
                self.num(depth),
                self.num(depth),
                self.rng.pick(&["==", "!=", "<", ">", "<=", ">="]),
            ),
            1 => (
                self.string(depth),
                self.string(depth),
                self.rng
                    .pick(&["==", "!=", "==#", "==?", "=~", "!~", "<", ">"]),
            ),
            2 => (
                self.list(depth),
                self.list(depth),
                self.rng.pick(&["==", "!=", "is", "isnot"]),
            ),
            _ => (
                self.dict(depth),
                self.dict(depth),
                self.rng.pick(&["==", "!=", "is", "isnot"]),
            ),
        };
        format!("({a} {op} {b})")
    }

    fn string(&mut self, depth: u32) -> String {
        if depth == 0 || self.rng.chance(30) {
            return match self.rng.below(3) {
                0 => self.var(Kind::Str),
                _ => self
                    .rng
                    .pick(&[
                        "''",
                        "'a'",
                        "'abc'",
                        "'a,b,,c'",
                        "'h\u{e9}llo'",
                        "'x1y22'",
                        "' pad '",
                        "\"tab\\t\"",
                        "'0x1F'",
                        "'-12'",
                    ])
                    .to_string(),
            };
        }
        let d = depth - 1;
        match self.rng.below(13) {
            0 => format!("({} . {})", self.string(d), self.scalar(d)),
            1 => format!("toupper({})", self.string(d)),
            2 => format!("string({})", self.any(d)),
            3 => format!(
                "strpart({}, {}, {})",
                self.string(d),
                self.num(d),
                self.num(d)
            ),
            4 => format!("repeat({}, {})", self.string(d), self.rng.below(4)),
            5 => {
                let pat = self
                    .rng
                    .pick(&["a", "\\d\\+", "\\w", ".", "^", "$", "\\(.\\)", "b*"]);
                let rep = self.rng.pick(&["X", "&&", "\\1", "[&]", "", "\\n"]);
                let flags = self.rng.pick(&["", "g"]);
                format!(
                    "substitute({}, '{pat}', '{rep}', '{flags}')",
                    self.string(d)
                )
            }
            6 => format!("join({}, {})", self.list(d), self.string(d)),
            7 => format!("{}[{}:{}]", self.string(d), self.num(d), self.num(d)),
            8 => format!("trim({})", self.string(d)),
            9 => format!(
                "matchstr({}, '{}')",
                self.string(d),
                self.rng.pick(&["\\d\\+", "[a-z]\\+", ".\\{2}"])
            ),
            10 => {
                let fmt = self
                    .rng
                    .pick(&["%d", "%s", "%5s|%-5s", "%x", "%05d", "%c", "%.2f"]);
                let n = fmt.matches('%').count();
                let args: Vec<String> = (0..n).map(|_| self.num(d)).collect();
                format!("printf('{fmt}', {})", args.join(", "))
            }
            11 => format!("tr({}, 'abc', 'xyz')", self.string(d)),
            _ => format!("nr2char({})", self.num(d)),
        }
    }

    fn list(&mut self, depth: u32) -> String {
        if depth == 0 || self.rng.chance(25) {
            return match self.rng.below(3) {
                0 => self.var(Kind::List),
                _ => "[]".into(),
            };
        }
        let d = depth - 1;
        match self.rng.below(16) {
            0 | 1 => {
                let n = self.rng.below(4);
                let items: Vec<String> = (0..n).map(|_| self.any(d)).collect();
                format!("[{}]", items.join(", "))
            }
            2 => format!("range({}, {})", self.num(d), self.num(d)),
            3 => format!(
                "split({}, '{}')",
                self.string(d),
                self.rng.pick(&[",", "\\d", ""])
            ),
            4 => format!("sort(copy({}))", self.list(d)),
            5 => format!("reverse(copy({}))", self.list(d)),
            6 => format!("{}[{}:{}]", self.list(d), self.num(d), self.num(d)),
            7 => format!("({} + {})", self.list(d), self.list(d)),
            8 => format!("map(copy({}), 'string(v:val)')", self.list(d)),
            9 => format!("filter(copy({}), 'v:val')", self.list(d)),
            10 => format!("deepcopy({})", self.list(d)),
            11 => format!("uniq(sort(copy({})))", self.list(d)),
            12 => format!(
                "{}({})",
                self.rng.pick(&["keys", "values", "items"]),
                self.dict(d)
            ),
            13 => format!("blob2list({})", self.blob(d)),
            14 => format!("str2list({})", self.string(d)),
            _ => format!("extendnew({}, {})", self.list(d), self.list(d)),
        }
    }

    fn dict(&mut self, depth: u32) -> String {
        if depth == 0 || self.rng.chance(25) {
            return match self.rng.below(3) {
                0 => self.var(Kind::Dict),
                _ => "{}".into(),
            };
        }
        let d = depth - 1;
        match self.rng.below(5) {
            0 | 1 => {
                let n = self.rng.below(3);
                let items: Vec<String> = (0..n)
                    .map(|_| {
                        format!(
                            "'{}': {}",
                            self.rng.pick(&["a", "b", "c", "1"]),
                            self.any(d)
                        )
                    })
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
            2 => format!("extend(copy({}), {})", self.dict(d), self.dict(d)),
            3 => format!("filter(copy({}), 'v:val')", self.dict(d)),
            _ => format!("deepcopy({})", self.dict(d)),
        }
    }

    fn blob(&mut self, depth: u32) -> String {
        if depth == 0 || self.rng.chance(25) {
            return match self.rng.below(3) {
                0 => self.var(Kind::Blob),
                _ => self
                    .rng
                    .pick(&["0z", "0z00", "0z0102", "0zDEADBEEF", "0zFF"])
                    .to_string(),
            };
        }
        let d = depth - 1;
        match self.rng.below(6) {
            0 => format!("({} + {})", self.blob(d), self.blob(d)),
            1 => format!("{}[{}:{}]", self.blob(d), self.num(d), self.num(d)),
            2 => format!("copy({})", self.blob(d)),
            3 => format!("list2blob({})", self.list(d)),
            4 => format!("reverse(copy({}))", self.blob(d)),
            _ => format!("repeat({}, {})", self.blob(d), self.rng.below(3)),
        }
    }

    fn float(&mut self, depth: u32) -> String {
        if depth == 0 || self.rng.chance(40) {
            return match self.rng.below(3) {
                0 => self.var(Kind::Float),
                _ => self
                    .rng
                    .pick(&["0.5", "1.5", "-2.25", "100.0", "0.0", "1.0e3"])
                    .to_string(),
            };
        }
        let d = depth - 1;
        match self.rng.below(5) {
            0 => format!("({} * {})", self.float(d), self.float(d)),
            1 => format!("({} / 2.0)", self.num(d)),
            2 => format!("floor({})", self.float(d)),
            3 => format!("round({})", self.float(d)),
            _ => format!("str2float({})", self.string(d)),
        }
    }

    /// A starting value for a variable of `kind`: literals only, so no
    /// initialiser reads a variable that is not defined yet.
    fn initial(&mut self, kind: Kind) -> String {
        let choices: &[&str] = match kind {
            Kind::Num => &["0", "1", "-3", "42", "255"],
            Kind::Str => &["''", "'abc'", "'a,b,,c'", "'h\u{e9}llo'", "'12'"],
            Kind::List => &[
                "[]",
                "[1, 2, 3]",
                "['a', 'b']",
                "[[1], [2, [3]]]",
                "[0, '', 1.5]",
            ],
            Kind::Dict => &["{}", "{'a': 1}", "{'a': [1], 'b': {'c': 2}}", "{'1': 'x'}"],
            Kind::Blob => &["0z", "0z00", "0z0102FF", "0zDEADBEEF"],
            Kind::Float => &["0.0", "1.5", "-2.25", "1.0e3"],
        };
        self.rng.pick(choices).to_string()
    }

    // ── statements ───────────────────────────────────────────────────────

    fn statement(&mut self, depth: u32) {
        match self.rng.below(30) {
            0..=3 => {
                let kind = self.any_kind();
                let name = self.var(kind);
                let e = self.expr(kind, 3);
                self.line(format!("let {name} = {e}"));
            }
            4 | 5 => {
                let (l, i, e) = (self.var(Kind::List), self.num(1), self.any(2));
                self.line(format!("let {l}[{i}] = {e}"));
            }
            6 => {
                let (l, a, b, e) = (self.var(Kind::List), self.num(1), self.num(1), self.list(2));
                self.line(format!("let {l}[{a}:{b}] = {e}"));
            }
            7 => {
                let (d, e) = (self.var(Kind::Dict), self.any(2));
                let key = self.rng.pick(&["a", "b", "c", "1"]);
                self.line(format!("let {d}['{key}'] = {e}"));
            }
            8 => {
                let (b, i, e) = (self.var(Kind::Blob), self.num(1), self.num(1));
                self.line(format!("let {b}[{i}] = {e}"));
            }
            9 => {
                let (b, a, c, e) = (self.var(Kind::Blob), self.num(1), self.num(1), self.blob(2));
                self.line(format!("let {b}[{a}:{c}] = {e}"));
            }
            10 | 11 => {
                let kind = self.any_kind();
                let name = self.var(kind);
                let op = self.rng.pick(&["+=", "-=", "*=", ".=", "..="]);
                let e = self.expr(kind, 2);
                self.line(format!("let {name} {op} {e}"));
            }
            12 => {
                let (l, i) = (self.var(Kind::List), self.num(1));
                self.line(format!("unlet {l}[{i}]"));
            }
            13 => {
                let d = self.var(Kind::Dict);
                let key = self.rng.pick(&["a", "b", "c", "1"]);
                self.line(format!("unlet! {d}['{key}']"));
            }
            14 | 15 => self.mutator(),
            16..=19 => {
                let n = 1 + self.rng.below(3);
                let args: Vec<String> = (0..n).map(|_| self.any(3)).collect();
                self.line(format!("echo {}", args.join(" ")));
            }
            20 | 21 if depth > 0 => self.if_block(depth - 1),
            22 if depth > 0 => self.for_block(depth - 1),
            23 if depth > 0 => self.while_block(depth - 1),
            24 | 25 if depth > 0 => self.try_block(depth - 1),
            26 if depth > 0 => self.function_block(depth - 1),
            27 => {
                let kind = self.any_kind();
                let name = self.var(kind);
                self.line(format!("lockvar {name}"));
            }
            28 => {
                let kind = self.any_kind();
                let name = self.var(kind);
                self.line(format!("unlockvar {name}"));
            }
            _ => {
                let e = self.any(2);
                self.line(format!("execute 'echo ' . string({e})"));
            }
        }
    }

    /// `:call` of a builtin that mutates its first argument in place.
    fn mutator(&mut self) {
        let text = match self.rng.below(9) {
            0 => format!("call add({}, {})", self.var(Kind::List), self.any(2)),
            1 => format!("call remove({}, {})", self.var(Kind::List), self.num(1)),
            2 => format!("call extend({}, {})", self.var(Kind::List), self.list(2)),
            3 => format!(
                "call insert({}, {}, {})",
                self.var(Kind::List),
                self.any(2),
                self.num(1)
            ),
            4 => format!("call sort({})", self.var(Kind::List)),
            5 => format!("call reverse({})", self.var(Kind::List)),
            6 => format!("call extend({}, {})", self.var(Kind::Dict), self.dict(2)),
            7 => format!("call remove({}, {})", self.var(Kind::Dict), self.string(1)),
            _ => format!("call add({}, {})", self.var(Kind::Blob), self.num(1)),
        };
        self.line(text);
    }

    fn block(&mut self, depth: u32) {
        self.indent += 1;
        for _ in 0..1 + self.rng.below(3) {
            self.statement(depth);
        }
        self.indent -= 1;
    }

    fn if_block(&mut self, depth: u32) {
        let cond = self.any(2);
        self.line(format!("if {cond}"));
        self.block(depth);
        if self.rng.chance(50) {
            self.line("else".into());
            self.block(depth);
        }
        self.line("endif".into());
    }

    fn for_block(&mut self, depth: u32) {
        let list = self.list(2);
        // A loop variable is plain `x`; the body may or may not touch `l*`.
        self.line(format!("for x in {list}"));
        self.indent += 1;
        self.line("echo x".into());
        self.indent -= 1;
        self.block(depth);
        self.line("endfor".into());
    }

    /// Bounded: the counter is never assigned by generated statements.
    fn while_block(&mut self, depth: u32) {
        self.fresh += 1;
        let c = format!("c{}", self.fresh);
        self.line(format!("let {c} = 0"));
        self.line(format!("while {c} < 3"));
        self.indent += 1;
        self.line(format!("let {c} += 1"));
        self.indent -= 1;
        self.block(depth);
        self.line("endwhile".into());
    }

    fn try_block(&mut self, depth: u32) {
        self.line("try".into());
        self.block(depth);
        if self.rng.chance(40) {
            let e = self.any(1);
            self.indent += 1;
            self.line(format!("throw {e}"));
            self.indent -= 1;
        }
        self.line("catch".into());
        self.indent += 1;
        self.line("echo 'caught' v:exception".into());
        self.indent -= 1;
        if self.rng.chance(40) {
            self.line("finally".into());
            self.block(depth);
        }
        self.line("endtry".into());
    }

    fn function_block(&mut self, depth: u32) {
        self.fresh += 1;
        let name = format!("Fn{}", self.fresh);
        let abort = if self.rng.chance(50) { " abort" } else { "" };
        self.line(format!("function! {name}(a, ...){abort}"));
        // A function sees no script variable by its bare name: copy them in (a
        // List, Dict or Blob stays the same object, so mutation still shows).
        self.indent += 1;
        for kind in KINDS {
            for i in 0..VARS_PER_KIND {
                let v = var_name(kind, i);
                self.line(format!("let {v} = g:{v}"));
            }
        }
        self.indent -= 1;
        self.block(depth);
        self.indent += 1;
        let ret = self.any(2);
        self.line(format!("return {ret}"));
        self.indent -= 1;
        self.line("endfunction".into());
        let arg = self.any(2);
        self.line(format!("echo {name}({arg})"));
        self.line(format!("echo {name}({arg}, 1, 2)"));
    }
}

/// The script for `seed`: initial values for every variable, a random body, and
/// a dump of the final state.
fn generate(seed: u64) -> String {
    let mut g = Gen {
        rng: Rng::new(seed),
        indent: 0,
        out: Vec::new(),
        fresh: 0,
        noise: env_u64("VIML_FUZZ_NOISE", 0) as usize,
    };
    for kind in KINDS {
        for i in 0..VARS_PER_KIND {
            let init = g.initial(kind);
            g.line(format!("let {} = {init}", var_name(kind, i)));
        }
    }
    for _ in 0..12 + g.rng.below(14) {
        g.statement(2);
    }
    for kind in KINDS {
        let names: Vec<String> = (0..VARS_PER_KIND)
            .map(|i| format!("string(get(g:, '{}', 'unset'))", var_name(kind, i)))
            .collect();
        g.line(format!("echo {}", names.join(" ")));
    }
    let mut text = g.out.join("\n");
    text.push('\n');
    text
}

// ── running a script ─────────────────────────────────────────────────────

const DEADLINE: Duration = Duration::from_secs(20);

/// The environment `scripts/parity.sh` pins for both engines.
fn pinned(mut cmd: Command) -> Command {
    for k in ["LC_CTYPE", "LC_COLLATE", "LC_TIME", "VIM", "VIMRUNTIME"] {
        cmd.env_remove(k);
    }
    cmd.env("LC_ALL", "C.UTF-8")
        .env("LANG", "C.UTF-8")
        .env("LC_MESSAGES", "C.UTF-8")
        .env("LANGUAGE", "")
        .env("TZ", "UTC");
    cmd
}

/// How a run ended: exit status (`None` for a signal or a timeout) and the
/// stdout+stderr stream in write order.
struct Outcome {
    status: Option<i32>,
    timed_out: bool,
    output: Vec<u8>,
}

fn run(mut cmd: Command) -> Outcome {
    let sink = tempfile::NamedTempFile::new().expect("capture file");
    let f = sink.reopen().expect("capture handle");
    let mut child = pinned_spawn(&mut cmd, f);
    let start = Instant::now();
    let (status, timed_out) = loop {
        match child.try_wait().expect("wait") {
            Some(s) => break (s.code(), false),
            None if start.elapsed() > DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    let output = fs::read(sink.path()).unwrap_or_default();
    Outcome {
        status,
        timed_out,
        output,
    }
}

fn pinned_spawn(cmd: &mut Command, sink: fs::File) -> std::process::Child {
    cmd.stdin(Stdio::null())
        .stdout(sink.try_clone().expect("dup capture handle"))
        .stderr(sink)
        .spawn()
        .expect("spawn")
}

fn run_viml(script: &Path) -> Outcome {
    let mut cmd = pinned(Command::new(env!("CARGO_BIN_EXE_viml")));
    cmd.arg(script);
    run(cmd)
}

fn run_editor(exe: &str, script: &Path, nvim: bool) -> Outcome {
    let mut cmd = pinned(Command::new(exe));
    let source = format!("verbose source {}", script.display());
    if nvim {
        cmd.args(["-es", "--clean", "-i", "NONE", "-c", &source, "-c", "qa!"]);
    } else {
        cmd.args([
            "-es", "-u", "NONE", "-i", "NONE", "-N", "-c", &source, "-c", "qa!",
        ]);
    }
    run(cmd)
}

/// `scripts/parity.sh`'s `norm()`: CRs go, and so do the `Error detected while
/// processing …:` / `line   N:` preamble lines (they embed the script's path).
fn normalise(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in raw.split(|&b| b == b'\n') {
        let line: Vec<u8> = line.iter().copied().filter(|&b| b != b'\r').collect();
        let text = String::from_utf8_lossy(&line);
        let is_preamble = text.starts_with("Error detected while processing ")
            || text.starts_with("Error executing ")
            || text.starts_with("Error in command line")
            || (text.starts_with("line")
                && text.trim_end().ends_with(':')
                && text[4..]
                    .trim_start()
                    .trim_end_matches(':')
                    .chars()
                    .all(|c| c.is_ascii_digit()));
        if !is_preamble {
            out.extend_from_slice(&line);
            out.push(b'\n');
        }
    }
    while out.last() == Some(&b'\n') {
        out.pop();
    }
    out
}

fn on_path(name: &str) -> Option<String> {
    let out = Command::new("sh")
        .args(["-c", &format!("command -v {name}")])
        .output()
        .ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !p.is_empty()).then_some(p)
}

// ── the tests ────────────────────────────────────────────────────────────

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
fn generator_is_deterministic_and_varied() {
    assert_eq!(generate(7), generate(7));
    let distinct: std::collections::HashSet<String> = (0..40).map(generate).collect();
    assert!(
        distinct.len() > 35,
        "generator collapsed to {} scripts",
        distinct.len()
    );
}

#[test]
fn generated_scripts_run_clean() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut bad: Vec<String> = Vec::new();
    for seed in 0..env_u64("VIML_FUZZ_CLEAN_COUNT", 60) {
        let script = dir.path().join(format!("s{seed}.vim"));
        fs::write(&script, generate(seed)).unwrap();
        let o = run_viml(&script);
        // 101 is a Rust panic; None is a signal or the deadline.
        if o.timed_out || o.status.is_none() || o.status == Some(101) {
            bad.push(format!(
                "seed {seed}: status {:?}{}\n{}",
                o.status,
                if o.timed_out { " (timed out)" } else { "" },
                String::from_utf8_lossy(&o.output)
            ));
        }
    }
    assert!(bad.is_empty(), "viml crashed or hung:\n{}", bad.join("\n"));
}

/// Whether `script` still shows a viml/oracle divergence — the predicate the
/// shrinker keeps true.
fn diverges(script: &Path, vim: &str, nvim: Option<&str>) -> Option<(Outcome, Outcome)> {
    let want = run_editor(vim, script, false);
    if want.timed_out {
        return None;
    }
    if let Some(nvim) = nvim {
        let second = run_editor(nvim, script, true);
        if normalise(&second.output) != normalise(&want.output) {
            return None; // the editors disagree: a dialect, not a finding
        }
    }
    let got = run_viml(script);
    let same = got.status == want.status && normalise(&got.output) == normalise(&want.output);
    (!same && !got.timed_out).then_some((want, got))
}

/// What a divergence is about: both exit statuses, and the `E<n>` numbers on
/// lines that only one side printed. A shrink step must preserve it — without
/// that, deleting a `:let` from the front of a script "still diverges" because
/// of the stray `:endif` it exposed, which is a different (and trivial) finding.
fn shape(want: &Outcome, got: &Outcome) -> String {
    fn lines(o: &Outcome) -> Vec<String> {
        String::from_utf8_lossy(&normalise(&o.output))
            .lines()
            .map(str::to_string)
            .collect()
    }
    let (a, b) = (lines(want), lines(got));
    let mut codes: Vec<String> = Vec::new();
    for (mine, other) in [(&a, &b), (&b, &a)] {
        for line in mine.iter().filter(|l| !other.contains(l)) {
            if let Some(i) = line
                .find('E')
                .filter(|&i| line[i + 1..].starts_with(|c: char| c.is_ascii_digit()))
            {
                let code: String = line[i..]
                    .chars()
                    .enumerate()
                    .take_while(|&(n, c)| n == 0 || c.is_ascii_digit())
                    .map(|(_, c)| c)
                    .collect();
                codes.push(code);
            }
        }
    }
    codes.sort();
    codes.dedup();
    format!("{:?}/{:?} {}", want.status, got.status, codes.join(","))
}

/// The line range a shrink step deletes at `i`: the whole block when `lines[i]`
/// opens one (through its closer at the same indent), otherwise that one line.
/// `else`/`catch`/`finally` are never candidates on their own.
fn removal_at(lines: &[String], i: usize) -> Option<std::ops::Range<usize>> {
    let text = lines[i].trim_start();
    let word = text
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("");
    if matches!(
        word,
        "else" | "catch" | "finally" | "endif" | "endfor" | "endwhile" | "endtry" | "endfunction"
    ) {
        return None;
    }
    let closer = match word {
        "if" => "endif",
        "for" => "endfor",
        "while" => "endwhile",
        "try" => "endtry",
        "function" => "endfunction",
        _ => return Some(i..i + 1),
    };
    let indent = lines[i].len() - text.len();
    let end = (i + 1..lines.len()).find(|&j| {
        lines[j].len() - lines[j].trim_start().len() == indent
            && lines[j].trim_start().starts_with(closer)
    })?;
    Some(i..end + 1)
}

/// How long one finding may be shrunk for. A reduction is a convenience, not a
/// result: what has been reduced so far is reported when the budget runs out.
const SHRINK_BUDGET: Duration = Duration::from_secs(120);

/// Delete statements (or whole blocks) while the SAME divergence survives, one
/// pass at a time, until a pass removes nothing or the budget is spent.
fn shrink(text: &str, dir: &Path, vim: &str, nvim: Option<&str>, target: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let probe = dir.join("shrink.vim");
    let started = Instant::now();
    loop {
        let mut removed = false;
        let mut i = 0;
        while i < lines.len() && started.elapsed() < SHRINK_BUDGET {
            let Some(range) = removal_at(&lines, i) else {
                i += 1;
                continue;
            };
            let mut cand = lines.clone();
            cand.drain(range);
            fs::write(&probe, cand.join("\n") + "\n").unwrap();
            match diverges(&probe, vim, nvim) {
                Some((want, got)) if shape(&want, &got) == target => {
                    lines = cand;
                    removed = true;
                }
                _ => i += 1,
            }
        }
        if !removed || started.elapsed() >= SHRINK_BUDGET {
            break;
        }
    }
    lines.join("\n") + "\n"
}

#[test]
#[ignore = "needs vim on PATH; run with --ignored"]
fn script_fuzz_matches_vim() {
    let Some(vim) = std::env::var("FUZZ_VIM").ok().or_else(|| on_path("vim")) else {
        eprintln!("script_fuzz_matches_vim: no vim on PATH, nothing to compare against");
        return;
    };
    let nvim = std::env::var("FUZZ_NVIM").ok().or_else(|| on_path("nvim"));
    let (count, first) = (
        env_u64("VIML_FUZZ_COUNT", 200),
        env_u64("VIML_FUZZ_SEED", 0),
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let mut findings: Vec<String> = Vec::new();

    for seed in first..first + count {
        let script = dir.path().join(format!("s{seed}.vim"));
        fs::write(&script, generate(seed)).unwrap();
        let Some((want, got)) = diverges(&script, &vim, nvim.as_deref()) else {
            continue;
        };
        let small = shrink(
            &generate(seed),
            dir.path(),
            &vim,
            nvim.as_deref(),
            &shape(&want, &got),
        );
        let probe = dir.path().join("final.vim");
        fs::write(&probe, &small).unwrap();
        let (want, got) = diverges(&probe, &vim, nvim.as_deref()).expect("shrink keeps divergence");
        findings.push(format!(
            "── seed {seed} ──\n{small}--- vim (status {:?}) ---\n{}\n--- viml (status {:?}) ---\n{}",
            want.status,
            String::from_utf8_lossy(&normalise(&want.output)),
            got.status,
            String::from_utf8_lossy(&normalise(&got.output)),
        ));
    }
    assert!(
        findings.is_empty(),
        "{} script(s) diverge from vim:\n\n{}",
        findings.len(),
        findings.join("\n\n")
    );
}

/// `VIML_FUZZ_SEED=<n> cargo test --test script_fuzz -- --ignored --nocapture
/// print_script` prints the script generated for one seed.
#[test]
#[ignore = "developer helper"]
fn print_script() {
    println!("{}", generate(env_u64("VIML_FUZZ_SEED", 0)));
}
