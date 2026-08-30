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

## Two modes

`--stmts` fuzzes **statements** instead of expressions. A snippet is wrapped in
`execute()`, which runs the commands and returns their output as a string, so it
rides the same three-engine pipeline — and any divergence in control flow, error
handling, or output shows up as a plain value mismatch.

This is not a nicety: the two worst bugs found in this interpreter (`:try` not
catching runtime errors, and a failed `:let` storing a corrupted value) live at the
statement level, and the expression-only fuzzer could not see either. The generator
covers user functions (arguments, defaults, varargs, closures), compound and
unpacking `:let`, indexed and member assignment, `:unlet`, `:for` over Lists, Dicts,
Strings and Blobs, `break`/`continue`, `while`, `:silent!`, `|`-separated command
lines, and both `:try` forms.

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
