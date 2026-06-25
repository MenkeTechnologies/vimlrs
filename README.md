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
| Value layer — `typval`, `list`, insertion-ordered `dict`, `blob` | Ported |
| Coercions + `typval_compare` + `num_divide`/`num_modulus` | Ported |
| `string()` / `:echo` rendering (`encode_tv2string`/`encode_tv2echo`) | Ported |
| Lexer / parser → AST (the `eval1`…`eval7` grammar) | Working |
| AST → fusevm bytecode lowering | Working |
| Expression engine — arithmetic, comparison, logic, ternary, index/slice, lists/dicts | Working |
| Builtin function surface | Partial (`len`/`type`/`string`/`empty`/`abs`/`str2nr`/`str2float`/`float2nr`; full `funcs.c` pending) |
| Standalone `vimlrs` binary (`-e` / `-c` / file / REPL) | Working |
| User functions, scopes (`l:`/`s:`/…), control flow (`:if`/`:while`/`:try`) | Planned |
| rkyv bytecode script cache | Planned |
| DAP debugger (`--dap`) · LSP server (`--lsp`) | Planned |

Porting discipline (exact C names, `// c:NNN` citations, two-zone `src/ported/`
vs crate-root carve-out layout) is documented in [`docs/PORT.md`](docs/PORT.md).

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
