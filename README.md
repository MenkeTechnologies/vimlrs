```
██╗   ██╗██╗███╗   ███╗██╗     ██████╗ ███████╗
██║   ██║██║████╗ ████║██║     ██╔══██╗██╔════╝
██║   ██║██║██╔████╔██║██║     ██████╔╝███████╗
╚██╗ ██╔╝██║██║╚██╔╝██║██║     ██╔══██╗╚════██║
 ╚████╔╝ ██║██║ ╚═╝ ██║███████╗██║  ██║███████║
  ╚═══╝  ╚═╝╚═╝     ╚═╝╚══════╝╚═╝  ╚═╝╚══════╝
```

[![CI](https://github.com/MenkeTechnologies/vimlrs/actions/workflows/ci.yml/badge.svg)](https://github.com/MenkeTechnologies/vimlrs/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/Rust-2021-05d9e8?style=flat-square)
![license](https://img.shields.io/badge/license-MIT-ff2a6d?style=flat-square)
![status](https://img.shields.io/badge/status-active%20%C2%B7%20in%20development-9b5de5?style=flat-square)

**VimL (Vimscript) in Rust** — the first compiled **standalone** VimL interpreter,
run outside Vim. A faithful port of Neovim's C eval engine, hosted on the
[`fusevm`](https://github.com/MenkeTechnologies/fusevm) bytecode VM with a
three-tier Cranelift JIT — the same engine behind `zshrs`, `stryke`, `awkrs`, and `elisp`.

## What it is

Vimscript has only ever run embedded inside Vim or Neovim. `vimlrs` takes the
eval engine out of the editor and runs `.vim` scripts as ordinary programs. The
language semantics are ported faithfully from Neovim's C `eval/*` tree — the C
source is the spec — rather than re-invented.

It is the fourth language hosted on `fusevm`. vimlrs carries no VM or JIT of its
own: it lexes and parses VimL to an AST, lowers that to fusevm bytecode, and lets
the shared engine run it — the same way `zshrs` hosts zsh.

```
VimL source  →  lexer  →  parser (AST)  →  lower to fusevm bytecode  →  fusevm VM + Cranelift JIT
```

## Status

In active development.

| Component | State |
|---|---|
| Value layer — `typval`, `list`, insertion-ordered `dict` (typed `tv_dict_get_*`/`tv_dict_add_*`, `tv_dict_add` fail-on-dup), `blob` | Ported |
| Coercions + `typval_compare` + `num_divide`/`num_modulus` | Ported |
| `string()` / `:echo` rendering (`encode_tv2string`/`encode_tv2echo`) | Ported |
| Lexer / parser → AST (the `eval1`…`eval7` grammar) | Working |
| AST → fusevm bytecode lowering | Working |
| Runs on fusevm's 3-tier Cranelift JIT | Working — JIT enabled; integer `+`/`-`/`*` → native `Op::Add`/`Sub`/`Mul`, integer compares → `Op::NumLt`/…; an integer expression **block-JIT-compiles** to machine code, and a function's numeric `while` loop (provably-Number `l:` locals → `Op::GetSlot`/`SetSlot`, loop rotated so the condition is the backedge) **trace-JIT-compiles** to native code — both verified by tests. Dynamic ops stay `CallBuiltin` (the deopt fallback). |
| Idiomatic `for i in range(N)` → native integer counter loop (no list built) that **trace-JIT-compiles** | Working (1/2/3-arg `range()`; the bound may be dynamic — `range(a:n)`/`range(len(x))` hoist a `tv_get_number`-coerced bound once in the prologue, so the body still traces; verified) |
| Numeric loops trace-JIT at **both function and script (top-level) scope** | Working — `slot_plan` slots provably-Number locals; explicit `l:name` refs in a function share the bare slot (`l:` *is* the local scope), while a name with a `g:`/`s:`/`a:`/… alias stays dict-backed |
| **Float** arithmetic + float-accumulator loops trace-JIT too (native `fadd`; int counter + float accumulator in one trace) | Working |
| Compound loop conditions (`&&`/`||` of numeric compares, short-circuit) trace-JIT; `if` inside loops + nested loops trace | Working |
| Per-loop slot scoping: a hot loop traces even when the function also calls helpers (callees can't see `l:` locals) or runs a sibling list-`for` | Working (function scope; script-scope calls still bail, since bare = `g:`) |
| Native integer `%` (e.g. `if i % 2 == 0`) so modulo loops trace; `/` stays on the builtin (fusevm div is float, unlike VimL integer `/`) | Working |
| Native numeric negation (`-x` → `Op::Negate`); `VIMLRS_JIT_STATS` counts function-body loops too | Working |
| Native bitwise builtins (`and`/`or`/`xor`/`invert` of integer args → `Op::BitAnd`/`BitOr`/`BitXor`/`BitNot`) so bit-manipulation loops trace | Working |
| Numeric ternary (`cond ? a : b`) — the test lowers through the native condition path and a numeric ternary is itself a Number, so `s += cond ? x : 0` loops trace | Working |
| Value-position comparison (`let s += i > 5`) — native compare reified to VimL's `0`/`1` with a branch (no `CallBuiltin`), so counting loops trace | Working |
| Logical-not of an integer (`!x` / `!(i % 2)`) → native `x == 0` reified to `0`/`1`, so it stays trace-eligible | Working |
| Observable from the real CLI: `VIMLRS_JIT_STATS=1 vimlrs script.vim` reports loop traces compiled; `VIMLRS_NO_JIT=1` forces the interpreter baseline | Working — a 20M-iteration loop runs **~15–100× faster** with the JIT |
| Native `Op::ReturnValue` (whole function bodies block-compile) + per-loop (not per-chunk) slot scoping | In progress (next) |
| Expression engine — arithmetic, comparison, logic, ternary, index/slice, lists/dicts | Working |
| Builtin function surface | Partial (`len`/`type`/`string`/`empty`/`abs`/`str2nr`/`str2float`/`float2nr`; full `funcs.c` pending) |
| Standalone `vimlrs` binary (`-e` / `-c` / file / `--repl`) | Working |
| Interactive REPL (`vimlrs --repl`, or bare `vimlrs` in a terminal) — reedline line editor with a live ASCII stats banner, Tab completion (the LSP wordlist), `~/.vimlrs/history`, and emacs/vi edit mode (`~/.vimlrs/config.toml` `[repl] mode`, `VIMLRS_REPL_MODE` override). Piped/non-TTY stdin falls back to the line-oriented reader. | Working |
| rkyv bytecode script cache (`~/.cache/vimlrs/scripts.rkyv`, mmap zero-copy) | Working |
| AOT build (`--build` bakes scripts into a self-contained executable) | Working |
| Bytecode disassembler (`--disasm`) | Working |
| LSP server (`--lsp`) — diagnostics, completion, hover, document symbols | Working |
| DAP debugger (`--dap`) — breakpoints, stepping, variables, evaluate | Working |
| Control flow — `:if`/`:elseif`/`:else`, `:while`, `:for`, `:break`/`:continue` | Working |
| `:execute`, `:let [a, b; rest] = …` & `:for [k, v] in …` destructuring | Working |
| `:let` compound assignment (`+=`/`-=`/`*=`/`/=`/`%=`/`.=`) — desugars to `target = target op rhs`, so accumulator loops trace-JIT | Working |
| `:let` index/member assignment (`let d['k']=v`, `let d.k=v`, `let l[i]=v`, compound forms) — Dict/List/Blob element set; Dict-set fires `dictwatcheradd()` watchers | Working |
| `\|` command separator (`let l = [1] \| echo l`) — strings/`\|\|`/`\\\|`/comment-aware | Working |
| User functions — `:function`/`:return`, recursion, `a:`/`l:` scopes | Working |
| vim9script foundation — `:vim9script` marker, `def NAME(p: type, …): rettype … enddef` with **bare** (a:-less) parameters + optional defaults, and vim9 automatic line continuation (unclosed `[]`/`{}`/`()`, leading/trailing binary operators, `->`/`.`/`?`/`:`, `#` comments) — `examples/vim9_def.vim` self-tests vs vim 9.2 | Working — type checking, bare-key `{k: v}` dict literals, `:class`, `import`/`export` deferred |
| Variable scopes — `g:`/`s:`/`b:`/`w:`/`t:`/`v:` + `:set`/`&opt` (`'ignorecase'` wired into regex) | Working |
| `:try`/`:catch`/`:finally`/`:throw` exceptions, `v:exception` | Working |
| `funcs.c` builtin table | In progress (~113 ported: string/list/dict, char-indexed string ops (`slice`/`strcharlen`/`strtrans`/`strwidth`/`strdisplaywidth`/`charclass`/`strutf16len`/`utf16idx`), `glob`/`globpath`, buffer/window introspection (`bufnr`/`winnr`/`tabpagenr`, editor-absent), float math + `isinf`/`isnan`, regex, `eval`/`execute`, `json_encode`/`json_decode`, env (`getenv`/`setenv`/`environ`), `system`/`systemlist` (shell out, sets `v:shell_error`), `shellescape`, `getpid`/`localtime`/`soundfold`, `reltime`/`reltimestr`/`reltimefloat`, `rand`/`srand` (xoshiro128**, bit-exact vs Neovim), `strftime`/`strptime`, `pathshorten`, `flattennew`, `sha256` (FIPS-180-2), `list2blob`/`blob2list` (+ blob index/slice), …) |
| `map`/`filter`/`sort`/`reduce`/`call` (lists **and** dicts; string-expr + funcref) | Working |
| Unit-testing framework — `assert_equal`/`assert_notequal`/`assert_true`/`assert_false`/`assert_match`/`assert_notmatch`/`assert_report`/`assert_inrange`/`assert_exception` → `v:errors`, plus `assert_fails` (run a command, require it to error/match a code) — message wording per `eval.lua` | Working — every `examples/*.vim` is a self-test, run in CI via `tests/examples.rs` |
| `eval()` / `execute()` (run-string metaprogramming) | Working |
| Regex engine — Vim magic dialect, backing `=~`/`matchstr`/`match`/`substitute`/`split`/`:catch` | Working |
| Regex char-class atoms are ASCII-only per `:help /\a` — `\a`/`\l`/`\u`/`\w`/`\d`/`\x` (+ negations) reject multibyte letters/digits (é, À, Ω, ４); only `\<`/`\>` word boundaries follow multibyte `'iskeyword'` — `examples/regex_classes.vim` self-tests vs nvim/vim | Working |
| Option-derived regex atoms — `\h`/`\H` head-of-word `[A-Za-z_]`, `\o`/`\O` octal `[0-7]` (true negations), plus `\p`/`\i`/`\k` from default `'isprint'`/`'isident'`/`'iskeyword'` with their `\P`/`\I`/`\K` "excluding-digits" forms (NOT set-complements, per `:help /\P`); `\p` is printable incl. multibyte, `\i` is single-byte only (é yes, Ω no), `\k` is multibyte-aware (é, Ω, 中) — `examples/regex_atoms.vim` self-tests vs nvim/vim. `\f`/`\F` (`'isfname'`) skipped: default is platform-conditional in Vim's C source | Working |
| POSIX bracket classes inside `[...]` per `:help /[:alpha:]` — the standard set (`[:alnum:]` `[:alpha:]` `[:blank:]` `[:cntrl:]` `[:digit:]` `[:graph:]` `[:lower:]` `[:print:]` `[:punct:]` `[:space:]` `[:upper:]` `[:xdigit:]`) plus Vim extras `[:tab:]`/`[:escape:]`/`[:backspace:]`/`[:return:]`/`[:ident:]`/`[:keyword:]`. ASCII-ness is not uniform: `[:alpha:]`/`[:alnum:]`/`[:digit:]`/`[:graph:]`/`[:punct:]` are ASCII-only, but `[:lower:]`/`[:upper:]` are Unicode-case-aware (é/À/Ω match, unlike ASCII-only `\l`/`\u`), `[:print:]` is multibyte-aware, and `[:space:]` includes vertical-tab (0x0B) which `\s` omits. Classes compose with ranges/literals and negate (`[[:digit:]a-f]`, `[^[:alpha:]]`). `[:fname:]` skipped (`'isfname'` platform-conditional, like `\f`) — `examples/regex_posix.vim` self-tests vs nvim/vim | Working |
| Case-fold (`\c`/`\C` and `'ignorecase'`) folds only LITERAL set members, not case-*defined* predicates — literal atoms (`\ca`), bracket literals (`[abc]`), ranges (`[A-Z]`/`[a-z]`, negated too) match either case under `\c`, but POSIX `[[:upper:]]`/`[[:lower:]]` and atoms `\u`/`\l` keep their definition (a lowercase char never matches `[[:upper:]]` under `\c`); case-agnostic `\d`/`\w`/`\a`/`\x` are no-ops and `\C` forces case-sensitive — `examples/regex_ic.vim` self-tests vs nvim/vim | Working |
| Substring/width builtins — byte-indexed `strpart` vs char-indexed `strcharpart`/`strgetchar`, `strlen`/`strchars`/`strwidth`/`strdisplaywidth`, `nr2char`/`char2nr` (multibyte + astral emoji round-trips) — `examples/substr_funcs.vim` / `examples/strwidth_funcs.vim` self-test vs nvim/vim | Working |
| String building — literal-pattern `substitute`, `tr`, `repeat` (string + list), `split` (literal sep + keepempty flag), `join` — `examples/strmanip.vim` self-tests vs nvim/vim | Working |
| `substitute()` Vim quirks — global empty-match handling per `do_string_sub`'s `zero_width` rule (an empty match at the previous empty-match position is skipped, so `a*` over `aaa` gives `X` not `XX`; `x*` over `abc` still gives `-a-b-c-`) + `vim_regsub` replacement specials where `\n` inserts a NUL (0x00), `\r` a carriage return, `\t` a tab, `\\` a backslash — `examples/substitute_edge.vim` self-tests vs nvim/vim | Working |
| `reduce()` left fold (seeded/unseeded, numeric/string/list accumulators) + positional list `extend(l, l2, idx)` — `examples/reduce_fold.vim` self-tests vs nvim/vim | Working |
| `printf()`/`%s`/`%S` stringify containers — a List/Dict/Funcref argument renders as its `string()` form (`[1, 2, 3]`, `{'a': 1}`, `type`) via `tv_str`→`encode_tv2echo` instead of raising E730 — `examples/printf_containers.vim` self-tests vs nvim/vim | Working |
| Value introspection — `type()`/`empty()`/`len()` across Number/String/Funcref/List/Dict/Float/Bool/Special/Blob; `abs`/`ceil`/`floor`/`trunc`/`round` (half-away-from-zero)/`float2nr` — `examples/typeintro.vim` / `examples/numround.vim` self-test vs nvim/vim | Working |
| Logical operators `&&`/`||` as normalised booleans (always 0/1, never an operand) with short-circuit, `!` boolean-coercion negation, and nested/chained `?:` ternaries — string truthiness follows numeric coercion (`'abc'`→false, `'0'`→false) — `examples/logic_ops.vim` self-tests vs nvim/vim | Working |
| Implicit String→Number coercion in arithmetic — leading-integer parse with `0x`/`0X`/`0b`/`0`(octal) prefixes, fractional/scientific tail dropped, non-numeric/whitespace-led → 0; plus `abs`/`min`/`max` over lists and dict-values — `examples/coerce_arith.vim` self-tests vs nvim/vim | Working |
| Byte-offset string search — `stridx`/`strridx` (literal, first/last, `{start}`), `match`/`matchend` (regex start/end offset), `matchstr` (`{start}`), `matchstrpos` (`['',-1,-1]` miss shape), `matchlist` — `examples/str_search.vim` self-tests vs nvim/vim | Working |
| `funcref()` builtin over user functions — direct call, `call()` interop, leading-List Partial pre-bind, and Funcrefs stored in List/Dict elements invoked via bracket index — `examples/funcref_builtin.vim` self-tests vs nvim/vim | Working |
| Date/time builtins — `strftime` formats an epoch second, `strptime` parses one back; TZ-independent `strptime`→`strftime` round-trips (full stamps, leap day, partial reformat), literal `%%`/empty format, and result-type checks — `examples/date_format.vim` self-tests vs nvim/vim | Working |
| Transcendental float math — `sqrt`/`exp`/`log`/`log10`, trig `sin`/`cos`/`tan` + inverse `asin`/`acos`/`atan`/`atan2` (quadrant-aware), hyperbolic `sinh`/`cosh`/`tanh`, verified via `float2nr(round(x*1e6))` fixed-point + forward/inverse identities — `examples/math_trig.vim` self-tests vs nvim/vim | Working |
| In-place list editing — `add`/`insert` (default-prepend + index), `remove` (single index, negative index, and index-range slice return), `count` (`{ic,start}` window), `index` (`{start,ic}`), and `range()` (`end`/`start,end`/`start,end,stride`) — `examples/list_edit.vim` self-tests vs nvim/vim | Working |
| String↔number conversion — `str2nr` (bases 2/8/10/16 with `0x`/`0b` prefixes, sign, leading space), `str2float` (decimal/scientific), `char2nr`/`nr2char` multibyte codepoint round-trips, `trim` (default whitespace, custom mask, `dir` leading/trailing), `escape` — `examples/str_numconv.vim` self-tests vs nvim/vim | Working |
| `:source {file}` (functions/globals persist) + autoload (`foo#bar()` sources `autoload/foo.vim` on demand) | Working |
| Lambdas `{args -> body}` (with closure capture), funcref-variable calls `F(args)`, Blob literals `0z…`, `d.key` member read, `#{key: val}` literal-key Dicts, `\` line continuation | Working |
| one-line block bars — `if … \| … \| endif` (and `for`/`while`), incl. after a leaf command (`let x=1 \| if x \| … \| endif`) | Working |
| variadic functions (`...` -> `a:000`/`a:0`), `:unlet`, `:source`, autoload | Working |
| `:command`/`:autocmd` (user commands + `:doautocmd` event firing) | Working — `examples/user_commands.vim` / `examples/autocommands.vim` self-test in CI |
| Blob index/slice operators + arithmetic — element index → unsigned byte, INCLUSIVE `[a:b]` sub-Blob (with open/negative ends), `+` concatenation, `==`/`!=` content compare, `string()` 0z-literal render, `type()` → `v:t_blob` (10), and `get()` byte-or-default — `examples/blob_bytes.vim` self-tests vs nvim/vim | Working |
| `sha256()` SHA-256 hex digest — FIPS-180-4 empty/`abc` vectors, longer ASCII, UTF-8 multibyte, 1000-byte multi-block, plus 64-char/lowercase/deterministic/avalanche invariants — `examples/sha256_digest.vim` self-tests vs nvim/vim | Working |
| Dictionary copy + `extend` collision policy — shallow `copy()` (nested containers shared) vs recursive `deepcopy()` (fully independent), `extend` `force`/`keep`/`error` (E737) actions, and `extendnew()` leaving both args intact — `examples/dict_deepcopy.vim` self-tests vs nvim/vim | Working |
| `count()` across container kinds — non-overlapping substring count on Strings, value-match count on Dicts, element count on Lists, with the `ic` case-fold flag and multibyte substrings — `examples/count_types.vim` self-tests vs nvim/vim | Working |

The full interpreter C surface is scaffolded: `scripts/gen_port_stubs.sh`
generates one stub per not-yet-ported Neovim C function (real name +
`vendor/<file>:<line>` citation) under `src/ported/stubs/`, so the remaining work
is enumerated and the drift gate covers it. Functions drop out of the stub tree
as they are faithfully ported.

Porting discipline (exact C names, `// c:NNN` citations, two-zone `src/ported/`
vs crate-root carve-out layout, the stub surface) is documented in the
Port methodology section of [`docs/report.html`](docs/report.html).

## Building

```sh
git clone https://github.com/MenkeTechnologies/vimlrs
cd vimlrs
cargo build
cargo test
```

`fusevm` is pulled from crates.io with the `jit` and `jit-disk-cache` features.
The vendored Neovim C eval sources under `vendor/` are the porting spec and are
excluded from the crate build.

## Links

- **Docs** — https://menketechnologies.github.io/vimlrs/
- **Engineering report** — https://menketechnologies.github.io/vimlrs/report.html
- **The shared VM** — [`fusevm`](https://github.com/MenkeTechnologies/fusevm)

## License

MIT. See [LICENSE](LICENSE).
