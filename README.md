```
██╗   ██╗██╗███╗   ███╗██╗     ██████╗ ███████╗
██║   ██║██║████╗ ████║██║     ██╔══██╗██╔════╝
██║   ██║██║██╔████╔██║██║     ██████╔╝███████╗
╚██╗ ██╔╝██║██║╚██╔╝██║██║     ██╔══██╗╚════██║
 ╚████╔╝ ██║██║ ╚═╝ ██║███████╗██║  ██║███████║
  ╚═══╝  ╚═╝╚═╝     ╚═╝╚══════╝╚═╝  ╚═╝╚══════╝
```

![Rust](https://img.shields.io/badge/Rust-2021-05d9e8?style=flat-square)
![license](https://img.shields.io/badge/license-MIT-ff2a6d?style=flat-square)
![status](https://img.shields.io/badge/status-early%20%C2%B7%20in%20development-9b5de5?style=flat-square)

**VimL (Vimscript) in Rust** — the first compiled **standalone** VimL interpreter,
run outside Vim. A faithful port of Neovim's C eval engine, hosted on the
[`fusevm`](https://github.com/MenkeTechnologies/fusevm) bytecode VM with a
three-tier Cranelift JIT — the same engine behind `zshrs`, `stryke`, and `awkrs`.

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

Early / in development.

| Component | State |
|---|---|
| Value layer — `typval`, `list`, insertion-ordered `dict` (typed `tv_dict_get_*`/`tv_dict_add_*`, `tv_dict_add` fail-on-dup), `blob` | Ported |
| Coercions + `typval_compare` + `num_divide`/`num_modulus` | Ported |
| `string()` / `:echo` rendering (`encode_tv2string`/`encode_tv2echo`) | Ported |
| Lexer / parser → AST (the `eval1`…`eval7` grammar) | Working |
| AST → fusevm bytecode lowering | Working |
| Runs on fusevm's 3-tier Cranelift JIT | Working — JIT enabled; integer `+`/`-`/`*` → native `Op::Add`/`Sub`/`Mul`, integer compares → `Op::NumLt`/…; an integer expression **block-JIT-compiles** to machine code, and a function's numeric `while` loop (provably-Number `l:` locals → `Op::GetSlot`/`SetSlot`, loop rotated so the condition is the backedge) **trace-JIT-compiles** to native code — both verified by tests. Dynamic ops stay `CallBuiltin` (the deopt fallback). |
| Idiomatic `for i in range(N)` → native integer counter loop (no list built) that **trace-JIT-compiles** | Working (1/2/3-arg `range()`; verified) |
| Numeric loops trace-JIT at **both function and script (top-level) scope** | Working — `slot_plan` slots provably-Number locals; explicit `l:name` refs in a function share the bare slot (`l:` *is* the local scope), while a name with a `g:`/`s:`/`a:`/… alias stays dict-backed |
| **Float** arithmetic + float-accumulator loops trace-JIT too (native `fadd`; int counter + float accumulator in one trace) | Working |
| Compound loop conditions (`&&`/`||` of numeric compares, short-circuit) trace-JIT; `if` inside loops + nested loops trace | Working |
| Per-loop slot scoping: a hot loop traces even when the function also calls helpers (callees can't see `l:` locals) or runs a sibling list-`for` | Working (function scope; script-scope calls still bail, since bare = `g:`) |
| Native integer `%` (e.g. `if i % 2 == 0`) so modulo loops trace; `/` stays on the builtin (fusevm div is float, unlike VimL integer `/`) | Working |
| Native numeric negation (`-x` → `Op::Negate`); `VIMLRS_JIT_STATS` counts function-body loops too | Working |
| Observable from the real CLI: `VIMLRS_JIT_STATS=1 vimlrs script.vim` reports loop traces compiled; `VIMLRS_NO_JIT=1` forces the interpreter baseline | Working — a 20M-iteration loop runs **~15–100× faster** with the JIT |
| Native `Op::ReturnValue` (whole function bodies block-compile) + per-loop (not per-chunk) slot scoping | In progress (next) |
| Expression engine — arithmetic, comparison, logic, ternary, index/slice, lists/dicts | Working |
| Builtin function surface | Partial (`len`/`type`/`string`/`empty`/`abs`/`str2nr`/`str2float`/`float2nr`; full `funcs.c` pending) |
| Standalone `vimlrs` binary (`-e` / `-c` / file / REPL) | Working |
| rkyv bytecode script cache (`~/.cache/vimlrs/scripts.rkyv`, mmap zero-copy) | Working |
| AOT build (`--build` bakes scripts into a self-contained executable) | Working |
| Bytecode disassembler (`--disasm`) | Working |
| LSP server (`--lsp`) — diagnostics, completion, hover, document symbols | Working |
| DAP debugger (`--dap`) — breakpoints, stepping, variables, evaluate | Working |
| Control flow — `:if`/`:elseif`/`:else`, `:while`, `:for`, `:break`/`:continue` | Working |
| `:execute`, `:let [a, b; rest] = …` & `:for [k, v] in …` destructuring | Working |
| `:let` compound assignment (`+=`/`-=`/`*=`/`/=`/`%=`/`.=`) — desugars to `target = target op rhs`, so accumulator loops trace-JIT | Working |
| `\|` command separator (`let l = [1] \| echo l`) — strings/`\|\|`/`\\\|`/comment-aware | Working |
| User functions — `:function`/`:return`, recursion, `a:`/`l:` scopes | Working |
| Variable scopes — `g:`/`s:`/`b:`/`w:`/`t:`/`v:` + `:set`/`&opt` (`'ignorecase'` wired into regex) | Working |
| `:try`/`:catch`/`:finally`/`:throw` exceptions, `v:exception` | Working |
| `funcs.c` builtin table | In progress (~106 ported: string/list/dict, char-indexed string ops, float math + `isinf`/`isnan`, regex, `eval`/`execute`, `json_encode`/`json_decode`, env (`getenv`/`setenv`), `shellescape`, `getpid`/`localtime`/`soundfold`, `reltime`/`reltimestr`/`reltimefloat`, `rand`/`srand` (xoshiro128**, bit-exact vs Neovim), …) |
| `map`/`filter`/`sort`/`reduce`/`call` (lists **and** dicts; string-expr + funcref) | Working |
| `eval()` / `execute()` (run-string metaprogramming) | Working |
| Regex engine — Vim magic dialect, backing `=~`/`matchstr`/`match`/`substitute`/`split`/`:catch` | Working |
| autoload (`foo#bar`), one-line block bars (`if … \| … \| endif`), `:source`/`:command`/`:autocmd` | Planned |

The full interpreter C surface is scaffolded: `scripts/gen_port_stubs.sh`
generates one stub per not-yet-ported Neovim C function (real name +
`csrc/<file>:<line>` citation) under `src/ported/stubs/`, so the remaining work
is enumerated and the drift gate covers it. Functions drop out of the stub tree
as they are faithfully ported.

Porting discipline (exact C names, `// c:NNN` citations, two-zone `src/ported/`
vs crate-root carve-out layout, the stub surface) is documented in
[`docs/PORT.md`](docs/PORT.md).

## Building

```sh
git clone https://github.com/MenkeTechnologies/vimlrs
cd vimlrs
cargo build
cargo test
```

`fusevm` is pulled from crates.io with the `jit` and `jit-disk-cache` features.
The vendored Neovim C eval sources under `csrc/` are the porting spec and are
excluded from the crate build.

## Links

- **Docs** — https://menketechnologies.github.io/vimlrs/
- **Engineering report** — https://menketechnologies.github.io/vimlrs/report.html
- **The shared VM** — [`fusevm`](https://github.com/MenkeTechnologies/fusevm)

## License

MIT. See [LICENSE](LICENSE).
