# Differential fuzzing — vimlrs vs. real Vim/Neovim

`fuzz-parity` generates random VimL expressions, runs them through vimlrs **and**
through `nvim` and `vim`, and reports every place the three disagree. It is how
parity gaps are found now; `BUGS.md` Round 5 is its output.

```sh
cargo run --bin fuzz-parity -- --count 1500 --seed 11
cargo run --bin fuzz-parity -- --only substitute,printf --count 500 --verbose
cargo run --bin fuzz-parity -- --count 2000 --seed 7 --corpus /tmp/gaps.txt
```

It needs `nvim` and `vim` on `PATH`, so it is a development tool and **never runs
in CI**. What CI runs is `tests/fuzz_corpus.rs`, which replays the fuzzer's
findings from `tests/data/fuzz_corpus.txt` in-process, with no editor installed.

## The oracle is an ENTRY POINT, not a binary

`vim` is not one reference editor. It is one binary with several dialects, chosen
by the flag vector it is launched with, and a harness that names only the binary
does not know which one it measured. Measured on vim 9.2 (patches 1-1000) by
replaying a driver of bare `&option` reads through two vectors that differ by one
flag:

| dropped flag | what moves |
|---|---|
| `-N` | 14 observables: `&compatible`, `&fileformats`, `&backspace`, `&whichwrap`, `&history`, `&viminfo`, `&formatoptions`, `&modeline`, `&shortmess`, `&more`, `&ruler`, `&showcmd`, `&hlsearch`, `&cedit` |
| `-i NONE` | vim READS **and WRITES** `~/.viminfo`, so one run mutates what the next one sees |

So both oracles are pinned, and both are printed before a single case is judged:

```text
fuzz-parity: oracle entry points
  nvim  /opt/homebrew/Cellar/neovim/0.12.5_1/bin/nvim
        NVIM v0.12.5
        argv: …/nvim --headless --clean -i NONE -S DRIVER.vim
        preflight: ok (encoding=utf-8, compatible pinned, ~/.viminfo untouched)
  vim   /opt/homebrew/Cellar/vim/9.2.1000/bin/vim
        VIM - Vi IMproved 9.2 (2026 Feb 14, compiled Aug 23 2026 19:45:42)
        argv: …/vim -es -u NONE -i NONE -N -S DRIVER.vim
        preflight: ok (encoding=utf-8, compatible pinned, ~/.viminfo untouched)
```

The path is absolute and symlink-resolved (`$FUZZ_VIM` / `$FUZZ_NVIM` override the
PATH lookup), the environment is pinned the way `scripts/parity.sh` pins it
(`LC_ALL=C.UTF-8`, `LANGUAGE=` cleared, `VIM`/`VIMRUNTIME` removed), and the
preflight *proves* the vector took: `&encoding` is `utf-8`, `&compatible` is 0,
and `~/.viminfo` is byte-for-byte unchanged after a launch. Any of the three
failing aborts the run rather than producing findings against an editor nobody
named.

`scripts/parity.sh` prints the same report (binary, sha256, version, argv, env,
observed state) and stamps it into `tests/parity_cases/ORACLE` when it re-records,
so a record taken through one entry point is never silently checked against
another.

## Shrinking

A grammar that nests produces findings nobody can read. Every distinct finding is
therefore reduced before it is reported: candidate cuts (delete a window of
characters at halving widths, unwrap one `\(…\)`, drop one `\|` branch, drop one
quantifier) are evaluated a whole round at a time — one process spawn per engine
per round, not per candidate — and the shortest one that still reproduces wins.

"Still reproduces" is the safety property, and it is strict: the reduced case must
still be a `GAP`/`PANIC`/`NEITHER` **and** keep the outcome shape engine by engine
— the same E-number where the original errored, a value where it valued. A looser
predicate reduced eight distinct regex findings to the single characters `,` `(`
`)` `:` `-`, each a real but entirely unrelated divergence. The outcomes printed
with a shrunk case are the ones measured **for the shrunk case**, never the
original's.

Shrinking is BOUNDED, because reduction is a convenience and not a result: at
most 40 distinct findings, at most 12 rounds and 25 seconds each, and the oracle
batches inside a shrink round abandon the rest of a batch once that deadline
passes. A pathological candidate costs a whole editor timeout and the batch loop
then restarts after it, so an unbounded shrinker did not finish 46 findings in an
hour. A missing oracle answer only means a candidate is not accepted as a
reduction — never that a finding is lost.

`--no-shrink` reports findings exactly as generated.

## Modes

`--stmts` fuzzes **statements** instead of expressions. A snippet is wrapped in
`execute()`, which runs the commands and returns their output as a string, so it
rides the same three-engine pipeline — and any divergence in control flow, error
handling, or output shows up as a plain value mismatch.

This is not a nicety: the two worst bugs found in this interpreter (`:try` not
catching runtime errors, and a failed `:let` storing a corrupted value) live at the
statement level, and the expression-only fuzzer could not see either. The generator
covers user functions (arguments, defaults, varargs, closures, recursion, funcref
round-trips, dict functions and `self`), compound and unpacking `:let`, indexed and
member assignment, nested targets, list-slice assignment, `:unlet`, `:const` and
the `:lockvar`/`:unlockvar` family, heredoc assignment (`=<<`, `=<< trim`), `:for`
over Lists, Dicts, Strings and Blobs, `break`/`continue`, `while`, `:elseif`
chains, `:silent!`, nested `execute()`, `|`-separated command lines, both `:try`
forms, `:finally`, pattern-matched `:catch`, re-throw from a catch, and `:echoerr`.

`--regex` fuzzes **patterns**, built from a grammar rather than a fixed list.
Alongside the atom/quantifier/alternation core it reaches the `\@` lookaround
family (`\@=`, `\@!`, `\@<=`, `\@<!`, `\@>`, and the bounded `\@3<=`), `\&`
branch conjunction, the collection edge cases (`[]-]`, `[^]]`, `[[=a=]]`), the
`\%…` buffer-position atoms, `~`, nested `\%[…]`, the omitted-bound quantifiers
(`\{,3}`, `\{2,}`, `\{}`), and magic switches in the MIDDLE of a pattern rather
than only at its front. Subjects include strings made of the pattern language's own
metacharacters (`*`, `**`, `a\b`, `[](){}`) — without those, a rule like "`*` just
after `^` is a literal star" is invisible, because every other subject answers the
same for the correct reading and the wrong one. Each pattern is checked through
`match`, `matchstr`, `matchend`, `matchlist`, `matchstrpos`, `substitute` (plain,
`g`, `&`, `\=submatch()`), and `split` (with and without `keepempty`).

`--dap` fuzzes the **debugger** rather than the language: it generates whole
programs and drives each through a live `viml --dap` session, stepping with a
seed-driven mix of verbs. A plain run of the program is its own oracle for
debugger drift and for the depth each step verb promises; a session that never
stops is counted separately and never as a pass.

## How a case is judged

Each expression is evaluated three times, and the verdict comes from comparing
all three results — not from comparing vimlrs to a single reference:

| class | condition | meaning |
|---|---|---|
| ok | vimlrs == oracle | parity |
| **GAP** | `nvim == vim`, vimlrs differs | a real bug — both references agree on the spec |
| **PANIC** | vimlrs panicked, crashed, or hung | a crash bug, always actionable |
| divergent | `nvim != vim` | Vim and Neovim disagree; advisory only, never counted against vimlrs |
| oracle-fail | neither engine answered | e.g. `range(9223372036854775807)` hangs Vim too — no spec, so no verdict |

Requiring **both** engines to agree before calling something a bug is what keeps
the report honest: Vim-vs-Neovim behavior splits (`get()` on `v:none`, `id()`,
`nr2char()` on an invalid codepoint, `"\<M-a>"`) show up as `divergent` instead of
masquerading as vimlrs defects.

Errors compare by **E-number only** (`E121`), never by message prose: the number
is Vim's stable contract, the wording is not.

## Determinism and safety

- **Seeded.** `--seed S` fixes the corpus; the same seed reproduces the same
  expressions on any machine, with no `rand` dependency (SplitMix64).
- **Fresh state per expression.** A prelude (`g:n`, `g:s`, `g:l`, `g:d`, `g:b`, …)
  is re-established before *every* expression in *every* engine, so a mutating
  call like `add(g:l, 4)` or `sort(g:l)` cannot leak into the next case.
- **Pure builtins only.** The generator draws from an allow-list of deterministic,
  non-blocking builtins — nothing touching the clock, filesystem, RNG, process
  table, or editor state. An impure builtin would report a false gap every run.
- **Sandboxed on both sides.** vimlrs runs in a child process under a 1 GiB heap
  cap and a wall-clock deadline, so a panic, an abort, a runaway allocation, or an
  infinite loop is *attributed to the exact expression that caused it* and the run
  continues. The oracles run in bounded chunks for the same reason: real Vim will
  also try to materialize `range(9223372036854775807)`, and a fuzzer that can take
  the machine down with it is not a fuzzer.

## Adding a finding to the CI gate

1. Reproduce it: `viml -c 'echo …'` next to `nvim --headless --clean -c 'echo …'`.
2. Fix the interpreter (port the C — `vendor/` is the spec).
3. Record the expectation **from the oracles**, never from vimlrs, and append it to
   `tests/data/fuzz_corpus.txt` as `<expr>TAB<expected>` (`!E123` for an error).
   Only record it when Vim and Neovim agree; if they differ, there is no spec to
   freeze — document it in `BUGS.md` instead.
4. `cargo test --test fuzz_corpus`.

Never edit a corpus expectation to match vimlrs. The file records what real Vim
does, which is the whole point of it.

## Read errors, do not capture them

The harness *observes* errors (`observe_error`) rather than calling
`capture_errors_begin`. Capturing is Vim's `emsg_silent` path, and a silenced error
is deliberately never converted into an exception (`cause_errthrow` declines) — so a
tool that captures in order to *read* errors silently disables `:try`/`:catch` in
everything it runs. That bug was real: the fuzzer once reported "vimlrs does not catch
runtime errors" for a dozen cases the binary catches perfectly well. A tool that
changes the behavior it measures is worse than no tool.

For the same reason the outcome is decided by `did_emsg` — the flag `:catch` resets
and `:silent!` never sets, i.e. the one that actually means "reported and unhandled" —
and the *first* such error is reported, since Vim raises one and abandons the command
while this VM keeps evaluating and can raise more.
