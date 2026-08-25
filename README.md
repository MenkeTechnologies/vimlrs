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

It is one of several languages hosted on `fusevm`. vimlrs carries no VM or JIT of its
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
| Observable from the real CLI: `VIMLRS_JIT_STATS=1 viml script.vim` reports loop traces compiled; `VIMLRS_NO_JIT=1` forces the interpreter baseline | Working — a 20M-iteration loop runs **~15–100× faster** with the JIT |
| Native `Op::ReturnValue` (whole function bodies block-compile) + per-loop (not per-chunk) slot scoping | In progress (next) |
| Expression engine — arithmetic, comparison, logic, ternary, index/slice, lists/dicts | Working |
| Builtin function surface | Working for the ported set — every name in the `BUILTIN_ARGC` table (`src/ported/eval/funcs_argc.rs`) dispatches and answers `exists('*name')`; the remainder of `funcs.c` is pending. The count is that table's row count, never a literal quoted here. |
| Standalone `viml` binary (`-e` / `-c` / file / `--repl`) | Working |
| Interactive REPL (`viml --repl`, or bare `viml` in a terminal) — reedline line editor with a live ASCII stats banner, Tab completion (the LSP wordlist), `~/.vimlrs/history`, and emacs/vi edit mode (`~/.vimlrs/config.toml` `[repl] mode`, `VIMLRS_REPL_MODE` override). Piped/non-TTY stdin falls back to the line-oriented reader. | Working |
| rkyv bytecode script cache (`~/.cache/vimlrs/scripts.rkyv`, mmap zero-copy) | Working |
| AOT build (`--build` bakes scripts into a self-contained executable) | Working |
| Bytecode disassembler (`--disasm`) | Working |
| Execution-tier report (`--tiers`) | Working |
| LSP server (`--lsp`) — diagnostics, completion, hover, document symbols | Working |
| DAP debugger (`--dap`) — line + function breakpoints, full backtrace, step in/over/out, variables, evaluate | Working |
| Control flow — `:if`/`:elseif`/`:else`, `:while`, `:for`, `:break`/`:continue`, and the abort rule: an error inside a conditional skips the rest of it out to the OUTERMOST open block (`ea.skip`'s `did_emsg` disjunct), so a `:while` whose body errors runs exactly once — unless the error is `:silent!`, caught, or raised inside a plain (non-`abort`) function body | Working — `tests/parity_cases/loop_error_abort.vim`, `loop_error_abort_guards.vim` |
| `:execute`, `:let [a, b; rest] = …` & `:for [k, v] in …` destructuring | Working |
| `:let` compound assignment (`+=`/`-=`/`*=`/`/=`/`%=`/`.=`) — the C's `eexe_mod_op` type table, not the expression operator: every combination it has no rule for is `E734: Wrong variable type for op=` and leaves the variable alone, and an environment variable, register or option carries its own extra rule. A provably-Number target still lowers to a native op, so accumulator loops trace-JIT | Working — `tests/parity_cases/let_compound_ops.vim` |
| `:let` index/member assignment (`let d['k']=v`, `let d.k=v`, `let l[i]=v`, compound forms) — Dict/List/Blob element set; Dict-set fires `dictwatcheradd()` watchers; writing into a locked container is `E741`/`E742` naming the lval as written, per `lockvar`'s depth | Working — `tests/parity_cases/lockvar_containers.vim` |
| `\|` command separator (`let l = [1] \| echo l`) — strings/`\|\|`/`\\\|`/comment-aware, and the two C reasons a failing command abandons the rest of the line: a REPORTED error (`ea.skip`), or a command that never set `eap->nextcmd` — a parse that aborted mid-expression, or a failed `:call` | Working — `tests/parity_cases/line_abandon.vim` |
| User functions — `:function`/`:return`, recursion, `a:`/`l:` scopes | Working |
| vim9script foundation — `:vim9script` marker, `def NAME(p: type, …): rettype … enddef` with **bare** (a:-less) parameters + optional defaults, and vim9 automatic line continuation (unclosed `[]`/`{}`/`()`, leading/trailing binary operators, `->`/`.`/`?`/`:`, `#` comments) — `examples/vim9_def.vim` self-tests vs vim 9.2 | Working — type checking, bare-key `{k: v}` dict literals, `:class`, `import`/`export` deferred |
| Variable scopes — `g:`/`s:`/`b:`/`w:`/`t:`/`l:`/`a:`/`v:` + `:set`/`&opt` (`'ignorecase'` wired into regex). Reading a scope dict whole (`keys(l:)`, `string(b:)`) lists every variable in it, `b:changedtick` included | Working — `tests/parity_cases/scopes_and_execute.vim`, `changedtick.vim` |
| `:try`/`:catch`/`:finally`/`:throw` exceptions, `v:exception` | Working |
| Error model — a builtin that reports an error still **returns its value**, so the command around it still runs (`echo str2nr('0x1f', 0)` prints `E474: Invalid argument` *and* `0`; `let g:r = strlen([1])` reports E730 and assigns `0`), while an *evaluator* failure (`eval5`'s operand pre-check, an unknown function, a bad subscript) abandons the command and leaves the variable alone. Inside a `:try` the error becomes an exception and then the command IS abandoned. The process exit status is `ex_exitval` — a one-way latch set only when a message is displayed, never reset by `:catch` | Working — `tests/parity_cases/builtin_error_value.vim` byte-diffs both halves against real vim |
| `substitute()` with a `\=` expression — a List is newline-joined with a trailing NL (`\=[1,2]` → `"1\n2\n"`, `\=[]` → `""`), any other List/Dict renders as its `string()` form, per `typval2string()`; a failed per-item `map()`/`filter()` callback leaves that item and every later one untouched | Working — `tests/parity_cases/substitute_expr_typval.vim`, `filter_map_callback_fail.vim` |
| `funcs.c` builtin table | In progress (string/list/dict, char-indexed string ops (`slice`/`strcharlen`/`strtrans`/`strwidth`/`strdisplaywidth`/`charclass`/`strutf16len`/`utf16idx`), `glob`/`globpath`, buffer/window introspection (`bufnr`/`winnr`/`tabpagenr`, editor-absent), float math + `isinf`/`isnan`, regex, `eval`/`execute`, `json_encode`/`json_decode`, env (`getenv`/`setenv`/`environ`), `system`/`systemlist` (shell out, sets `v:shell_error`), `shellescape`, `getpid`/`localtime`/`soundfold`, `reltime`/`reltimestr`/`reltimefloat`, `rand`/`srand` (xoshiro128**, bit-exact vs Neovim), `strftime`/`strptime`, `pathshorten`, `flattennew`, `sha256` (FIPS-180-2), `list2blob`/`blob2list` (+ blob index/slice), …) |
| `map`/`filter`/`sort`/`reduce`/`call` (lists **and** dicts; string-expr + funcref) | Working |
| Unit-testing framework — `assert_equal`/`assert_notequal`/`assert_true`/`assert_false`/`assert_match`/`assert_notmatch`/`assert_report`/`assert_inrange`/`assert_exception` → `v:errors`, plus `assert_fails` (run a command, require it to error/match a code) — message wording per `eval.lua`. Every entry carries the `prepare_assert_error()` location stamp (`<script>[N]..function F[N]..G line N: `), values render through `ga_concat_shorten_esc` (C0 controls and `\` escaped, a run of >20 identical characters collapsed to `\[c occurs N times]`), and a Dict/Dict `assert_equal` reports only the differing keys plus `- N equal items omitted` | Working — every `examples/*.vim` is a self-test, run in CI via `tests/examples.rs`; `tests/parity_cases/assert_location.vim`, `assert_escape.vim` and `assert_dict_diff.vim` byte-diff the messages against real vim |
| `eval()` / `execute()` (run-string metaprogramming) | Working |
| Regex engine — Vim magic dialect, backing `=~`/`matchstr`/`match`/`substitute`/`split`/`:catch` | Working |
| Regex char-class atoms are ASCII-only per `:help /\a` — `\a`/`\l`/`\u`/`\w`/`\d`/`\x` (+ negations) reject multibyte letters/digits (é, À, Ω, ４); only `\<`/`\>` word boundaries follow multibyte `'iskeyword'` — `examples/regex_classes.vim` self-tests vs nvim/vim | Working |
| Option-derived regex atoms — `\h`/`\H` head-of-word `[A-Za-z_]`, `\o`/`\O` octal `[0-7]` (true negations), plus `\p`/`\i`/`\k` from default `'isprint'`/`'isident'`/`'iskeyword'` with their `\P`/`\I`/`\K` "excluding-digits" forms (NOT set-complements, per `:help /\P`); `\p` is printable incl. multibyte, `\i` is single-byte only (é yes, Ω no), `\k` is multibyte-aware (é, Ω, 中) — `examples/regex_atoms.vim` self-tests vs nvim/vim. `\f`/`\F` (`'isfname'`) skipped: default is platform-conditional in Vim's C source | Working |
| POSIX bracket classes inside `[...]` per `:help /[:alpha:]` — the standard set (`[:alnum:]` `[:alpha:]` `[:blank:]` `[:cntrl:]` `[:digit:]` `[:graph:]` `[:lower:]` `[:print:]` `[:punct:]` `[:space:]` `[:upper:]` `[:xdigit:]`) plus Vim extras `[:tab:]`/`[:escape:]`/`[:backspace:]`/`[:return:]`/`[:ident:]`/`[:keyword:]`. ASCII-ness is not uniform: `[:alpha:]`/`[:alnum:]`/`[:digit:]`/`[:graph:]`/`[:punct:]` are ASCII-only, but `[:lower:]`/`[:upper:]` are Unicode-case-aware (é/À/Ω match, unlike ASCII-only `\l`/`\u`), `[:print:]` is multibyte-aware, and `[:space:]` includes vertical-tab (0x0B) which `\s` omits. Classes compose with ranges/literals and negate (`[[:digit:]a-f]`, `[^[:alpha:]]`). `[:fname:]` skipped (`'isfname'` platform-conditional, like `\f`) — `examples/regex_posix.vim` self-tests vs nvim/vim | Working |
| Case-fold (`\c`/`\C` and `'ignorecase'`) folds only LITERAL set members, not case-*defined* predicates — literal atoms (`\ca`), bracket literals (`[abc]`), ranges (`[A-Z]`/`[a-z]`, negated too) match either case under `\c`, but POSIX `[[:upper:]]`/`[[:lower:]]` and atoms `\u`/`\l` keep their definition (a lowercase char never matches `[[:upper:]]` under `\c`); case-agnostic `\d`/`\w`/`\a`/`\x` are no-ops and `\C` forces case-sensitive — `examples/regex_ic.vim` self-tests vs nvim/vim | Working |
| The `\@` lookaround family per `:help /\@=` — `\@=`/`\@!` zero-width (negative) lookahead, `\@<=`/`\@<!` lookbehind (atom must match *ending exactly at* the position; may match empty; farthest start wins captures; `\@123<=` limit form), `\@>` atomic group (standalone match, no backtracking into it); groups inside a successful positive lookahead are captured; bare `@` operator under `\v`; E866/E869/E871 rejections match the NFA engine — `examples/regex_look.vim` self-tests vs nvim/vim | Working |
| Substring/width builtins — byte-indexed `strpart` vs char-indexed `strcharpart`/`strgetchar`, `strlen`/`strchars`/`strwidth`/`strdisplaywidth`, `nr2char`/`char2nr` (multibyte + astral emoji round-trips) — `examples/substr_funcs.vim` / `examples/strwidth_funcs.vim` self-test vs nvim/vim | Working |
| Byte-exact strings — a VimL string is `char_u *`, so a byte-indexed cut (`s[i]`, `s[a:b]`, `strpart`, `strcharpart`) keeps the raw byte it landed on even when that byte splits a character, and two different split bytes stay two different strings (`s[1] ==# s[2]` is 0, `char2nr` answers the byte). `\x`/`\X`/octal escapes resolve to a raw byte (`len("\xc3")` is 1, `"\303\251"` is `'é'`) while `\u`/`\U` encode a code point, including above the Unicode maximum; a byte that resolves to 0 ends the string | Working — `tests/parity_cases/byte_exact_substring.vim`, `string_escape_bytes.vim` |
| Strict number literals — an alphanumeric glued to a literal is a parse failure at the literal (`12abc`, `1e5`, `0xg`, `007a` → `E15: Invalid expression: "…"`), per `vim_str2nr`'s `strict` flag; `_` still terminates it normally. The E15 quotes from the literal when the branch is evaluated and the whole expression when it is not | Working — `tests/parity_cases/number_literal_junk.vim` |
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
| Lambdas `{args -> body}` — a lambda body is an EXPRESSION with its own stored text, so a diagnostic raised inside one quotes only up to the `}` and a body that failed to evaluate makes the call yield -1 (`tests/parity_cases/lambda_body_diagnostics.vim`) — with closure capture, funcref-variable calls `F(args)`, Blob literals `0z…`, `d.key` member read, `#{key: val}` literal-key Dicts, `\` line continuation | Working |
| one-line block bars — `if … \| … \| endif` (and `for`/`while`), incl. after a leaf command (`let x=1 \| if x \| … \| endif`) | Working |
| variadic functions (`...` -> `a:000`/`a:0`/`a:1`…, `a:firstline`/`a:lastline`, every a: item read-only and `a:000` a FIXED list), `:unlet`, `:source`, autoload | Working — `tests/parity_cases/func_args_scope.vim` |
| Buffer model — one unnamed buffer and one window, as vim starts with. `getline`/`setline`/`append`/`deletebufline`/`getbufline`, the `buf*()` family (`bufnr`, `bufname`, `bufexists`, `buflisted`, `bufloaded`, `bufwinnr`, `bufwinid`), `getbufvar`/`setbufvar` and their window/tab pairs, `b:changedtick` counting its own edits, and the cursor moving with the text around it | Working — `tests/parity_cases/buffer_functions.vim`, `changedtick.vim`, `cursor_across_edits.vim`; the builtins that measure a TERMINAL (`winwidth`, `winheight`, `winrestcmd`) still answer -1/'' |
| Ex-command name table — all 564 built-in commands in the C's own order plus the 24 command modifiers, so `exists(':cmd')` and `fullcommand()` resolve abbreviations by table precedence (`:s` is `substitute`, `:co` is `copy`, `:con` is `continue`) | Working — `tests/parity_cases/cmd_exists_table.vim`; this port carries Neovim's table, so the nvim-only and vim-only command names differ by construction |
| `:command`/`:autocmd` (user commands + `:doautocmd` event firing) | Working — `examples/user_commands.vim` / `examples/autocommands.vim` self-test in CI |
| Blob index/slice operators + arithmetic — element index → unsigned byte, INCLUSIVE `[a:b]` sub-Blob (with open/negative ends), `+` concatenation, `==`/`!=` content compare, `string()` 0z-literal render, `type()` → `v:t_blob` (10), and `get()` byte-or-default — `examples/blob_bytes.vim` self-tests vs nvim/vim | Working |
| `sha256()` SHA-256 hex digest — FIPS-180-4 empty/`abc` vectors, longer ASCII, UTF-8 multibyte, 1000-byte multi-block, plus 64-char/lowercase/deterministic/avalanche invariants — `examples/sha256_digest.vim` self-tests vs nvim/vim | Working |
| Dictionary copy + `extend` collision policy — shallow `copy()` (nested containers shared) vs recursive `deepcopy()` (fully independent), `extend` `force`/`keep`/`error` (E737) actions, and `extendnew()` leaving both args intact — `examples/dict_deepcopy.vim` self-tests vs nvim/vim | Working |
| `count()` across container kinds — non-overlapping substring count on Strings, value-match count on Dicts, element count on Lists, with the `ic` case-fold flag and multibyte substrings — `examples/count_types.vim` self-tests vs nvim/vim | Working |
| AOP command-intercept extension (vimlrs/zshrs-original; **no Vim counterpart**) — before/after/around advice on user-function calls. `:Intercept before\|after\|around {pattern} { code }` (glob `*`/`?`/`all` on the function name) plus `:Intercept list\|remove {id}\|clear`, or the `intercept({kind}, {pattern}, {code})` / `intercept_proceed()` builtins. Around advice calls `intercept_proceed()` (or `:Intercept proceed`) to run the original and reuse its return value, or suppresses it. Advice is VimL evaluated in the current interpreter (no subprocess) with the AOP context in `g:INTERCEPT_NAME` / `g:INTERCEPT_ARGS` / `g:INTERCEPT_CMD` / `g:INTERCEPT_MS` / `g:INTERCEPT_US`. Ported from zshrs's `intercept` engine. | Working |

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

`fusevm` is pulled from crates.io; the enabled feature list is in `Cargo.toml`'s
dependency row for it, not restated here.
The vendored Neovim C sources under `vendor/` are the porting spec and are
excluded from the crate build. They are the eval tree plus the files the eval
tree calls out to that have observable behaviour of their own — `mbyte.c`,
`hashtab.c`, `charset.c` and `message.c` among them. `charset.c`/`message.c`
carry the display transform (`transchar` / `msg_outtrans`) that decides how
`:echo` and `strtrans()` render a byte a terminal cannot show.

## Parity testing

Parity with real Vim is checked three ways.

Every `examples/*.vim` is a self-testing script whose assertions were written
against Vim 9.2 / Neovim 0.12; `cargo test` runs all of them.

`scripts/parity.sh` is the **script-level differential harness**: it sources a
whole `.vim` file through vimlrs *and* through the real `vim` and byte-diffs the
captured output and the exit status. That is the level at which the divergences
between statements live — the message-column model (`:echo` vs `:echon` vs
`echo ''`), a `:set ignorecase` leaking into later comparisons, a `:const` or
`:function d.key()` that parses but leaves nothing behind.

```sh
bash scripts/parity.sh                     # the committed corpus
bash scripts/parity.sh probe.vim           # one ad-hoc script
bash scripts/parity.sh -r tests/parity_cases   # re-record from vim after a fix
```

Only vim ever writes an expectation, so a case cannot be made to pass by changing
vimlrs. The corpus is replayed against the recorded output by
`tests/parity_cases.rs`, which needs no editor installed and therefore runs in CI.

Both engines are run with a pinned environment — `LC_ALL=C.UTF-8`,
`LANGUAGE=` cleared, `TZ=UTC`, and `VIM`/`VIMRUNTIME` unset — so a record is the
language's answer rather than the shell of whoever re-recorded it. Without it,
`LC_ALL=C` moves every byte-level record (vim falls back to `encoding=latin1`)
and any locale vim ships a translation for moves every record containing an
`E<number>`. The harness verifies the pin took (it refuses to run if vim comes up
with anything but `encoding=utf-8`) because a locale the C library does not have
falls back to `C` silently.

On top of that, `fuzz-parity` is a **differential fuzzer**: it generates random
VimL expressions and runs each through vimlrs *and* through `nvim` *and* `vim`.
It reports a bug when both engines agree and vimlrs differs — so a Vim-vs-Neovim
behavior split is never mistaken for a vimlrs defect — and, separately, when the
two engines disagree and vimlrs matches *neither*. That second bucket is a bug
too: two references that disagree still bracket the answer between them, and a
third result outside that bracket is vimlrs's own. Only "the engines disagree and
vimlrs matches one of them" is advisory.

```sh
cargo run --bin fuzz-parity -- --count 1500 --seed 11
cargo run --bin fuzz-parity -- --dap --count 60 --seed 11
```

`--dap` fuzzes the **debugger** rather than the language: it generates whole
programs — nested user functions, branches, loops, `|` groups — and drives each
through a live `viml --dap` session, stepping with a seed-driven mix of verbs.
Two of its three findings need no editor at all, because the program is its own
oracle: whatever it prints, it must print the same under the debugger
(`DEBUG DRIFT`), and every stop must produce a backtrace whose outermost frame
is the script and a step that honours the depth its verb promised
(`INVARIANTS`). The third compares the plain run against vim as usual. A session
that never stops is counted separately and never as a pass, so a run that
reaches nothing cannot read as a clean one.

It needs both editors on `PATH`, so it is a development tool and does not run in
CI. Its findings are frozen into `tests/data/fuzz_corpus.txt` as
oracle-recorded expectations and replayed in-process by `tests/fuzz_corpus.rs`,
which needs no editor installed. `BUGS.md` records every divergence both harnesses
have found and what remains open.

## Links

- **Docs** — https://menketechnologies.github.io/vimlrs/
- **Reference** — https://menketechnologies.github.io/vimlrs/reference.html
- **Engineering report** — https://menketechnologies.github.io/vimlrs/report.html
- **The shared VM** — [`fusevm`](https://github.com/MenkeTechnologies/fusevm)

## License

MIT. See [LICENSE](LICENSE).
