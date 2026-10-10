# vimlrs JetBrains Plugin

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![IDE](https://img.shields.io/badge/IDE-2025.2%2B-orange.svg)](https://plugins.jetbrains.com/)
[![JDK](https://img.shields.io/badge/JDK-17-blue.svg)](https://adoptium.net/)
[![Plugin SDK](https://img.shields.io/badge/IntelliJ%20Platform%20Gradle-2.16-purple.svg)](https://plugins.jetbrains.com/docs/intellij/tools-intellij-platform-gradle-plugin.html)

### `[FULL IDE FRONT-END FOR THE STANDALONE VimL INTERPRETER]`

> *"Vimscript, without Vim — now with breakpoints."*

## `[BUILT FOR VIMLRS]`

A JetBrains-platform plugin that drives the LSP and DAP servers compiled into the `viml` binary — a standalone VimL (Vimscript) interpreter on the fusevm bytecode VM. Hand-rolled lexer for instant highlighting, completion, hover and diagnostics from the LSP, a breakpoint debugger over DAP, and run configs that auto-create from any `.vim` / vimrc-family file. Talks to the in-tree `src/lsp.rs` + `src/dap.rs` over JSON-RPC.

### [`vimlrs`](https://github.com/MenkeTechnologies/vimlrs) · [`fusevm`](https://github.com/MenkeTechnologies/fusevm) · [`strykelang`](https://github.com/MenkeTechnologies/strykelang)

---

## Table of Contents

- [\[0x00\] Overview](#0x00-overview)
- [\[0x01\] Install](#0x01-install)
- [\[0x02\] Editor](#0x02-editor)
- [\[0x03\] LSP](#0x03-lsp)
- [\[0x04\] Code Actions](#0x04-code-actions)
- [\[0x05\] Run / Debug](#0x05-run--debug)
- [\[0x06\] DAP Protocol](#0x06-dap-protocol)
- [\[0x07\] Refactor / Rename](#0x07-refactor--rename)
- [\[0x08\] Configuration](#0x08-configuration)
- [\[0x09\] Logs](#0x09-logs)
- [\[0x0A\] Building](#0x0a-building)
- [\[0x0B\] Plugin Architecture](#0x0b-plugin-architecture)
- [\[0x0C\] Version Compatibility](#0x0c-version-compatibility)
- [\[0x0D\] Limitations](#0x0d-limitations)
- [\[0xFF\] License](#0xff-license)

---

## [0x00] OVERVIEW

vimlrs ships an **LSP server** and **DAP debug adapter** built into the `viml` binary (`viml --lsp`, `viml --dap`, both over stdio). This plugin is the JetBrains-side driver:

- Spawns the LSP / DAP servers on demand, frames JSON-RPC over stdio, and renders responses through the IDE's native UI affordances (gutter breakpoints, completion popup, hover, diagnostics).
- Adds **zero new language code paths**. Highlighting comes from the hand-rolled `VimlrsLexer.kt`; completion, hover, document symbols and diagnostics come from the LSP server.
- On the Rust side the LSP server is built on the `lsp-server` / `lsp-types` crates; the DAP server is hand-framed JSON-RPC on top of `serde_json`. JetBrains' own `LspServerSupportProvider` is the plugin-side LSP client.

---

## [0x01] INSTALL

```sh
# Install from disk: Settings → Plugins → ⚙ → Install Plugin from Disk…
# Then pick:
editors/intellij/build/distributions/vimlrs-intellij-<version>.zip
```

After install: restart the IDE → open any `.vim` file (or `vimrc` / `.vimrc` / `_vimrc` / `gvimrc` / `.gvimrc` / `.exrc` / `init.vim`) → the LSP starts automatically → the debugger activates the first time you click Debug.

The `viml` binary must be on `$PATH`, or configured under *Settings → Tools → Vimlrs → vimlrs executable*. The plugin resolves the executable via `VimlrsSettings.vimlrsExecutable` first, then falls back to a `$PATH` lookup of `viml`.

---

## [0x02] EDITOR

| Surface | Behavior |
|---------|----------|
| File association | `.vim` plus the `vimrc` / `gvimrc` / `exrc` / `init.vim` family (configurable; see [§0x08](#0x08-configuration)) |
| Lexer | Hand-rolled in `VimlrsLexer.kt` — instant highlighting; the server does not provide semantic tokens |
| Color slots | One stable `VIMLRS_*` `TextAttributesKey` per token category under *Settings → Editor → Color Scheme → vimlrs* |
| Brace matching | `{` / `}`, `(` / `)`, `[` / `]` via `VimlrsBraceMatcher.kt` |
| Comments | Cmd/Ctrl-`/` for `"` line comments via `VimlrsCommenter.kt` (VimL has no block-comment form) |
| Quote handler | `"` and `'` auto-pair; inside-string typing recognized via `VimlrsQuoteHandler.kt` |
| Complete Current Statement | Cmd-Shift-Enter closes `if`/`while`/`for`/`function`/`try` blocks and balances brackets via `VimlrsSmartEnterProcessor.kt` |

### Lexer coverage

| Token category | Examples |
|----------------|----------|
| Comments | `"` line (command position only), `#!` shebang on line 1 |
| Strings | `"…"` (backslash escapes), `'…'` (literal, `''` escapes) |
| Numbers | `42`, `3.14`, `0x1F`, `0b1010`, `1.0e3` |
| Keywords | `if` / `elseif` / `else` / `endif` / `while` / `for` / `function` / `endfunction` / `try` / `catch` / `let` / `call` / `echo` / `return` … |
| Ex commands | `set` / `setlocal` / `autocmd` / `augroup` / `nnoremap` / `highlight` / `syntax` / `source` / `silent` … |
| Scope vars | `g:` `s:` `b:` `w:` `t:` `l:` `a:` `v:` followed by a name |
| Specials | `v:true` / `v:false` / `v:count` / `v:val` / `v:shell_error` / `v:exception` … |
| Options / env / register | `&number` / `&l:textwidth`, `$HOME`, `@a` |
| Builtin functions | `len(` / `has(` / `printf(` / `substitute(` (only before `(`) |
| Autoload | `plug#begin(` (colored as a declaration) |
| Operators | `==` `!=` `=~` `!~` (with `#` / `?` case flags), `..` `->` `+=` `-=` `.=`, `|` bar, `\` line continuation |

---

## [0x03] LSP

The LSP server is in-process inside the `viml` binary — `viml --lsp` spawns it over stdio. Plugin side starts it via `VimlrsLspServerSupportProvider.kt`; descriptor in `VimlrsLspServerDescriptor.kt`.

### Capabilities

| Capability | Trigger / scope |
|------------|-----------------|
| `completion` | builtins, ex commands, `v:` variables |
| `hover` | signature and description cards for the same names |
| `documentSymbol` | symbols declared in the open document |
| `publishDiagnostics` | per-line parse errors on `didOpen` / `didChange`; cleared on `didClose` |

### Transport

- **Stdio**, Content-Length-framed JSON-RPC via the `lsp-server` / `lsp-types` crates.
- Any other request is answered with `MethodNotFound`; definition, references, folding, rename, formatting, semantic tokens and code actions are not implemented.

---

## [0x04] CODE ACTIONS

The plugin routes the IntelliJ Refactor menu (Ctrl-T) through `VimlrsRefactoringSupportProvider.kt` to LSP code-action requests for Extract Method / Variable / Constant. The server advertises no code-action capability, so these requests currently find no action. Failure modes (no LSP, no matching action) surface as balloon notifications instead of silent dead keys.

---

## [0x05] RUN / DEBUG

### Run

| Surface | Behavior |
|---------|----------|
| **Run config** (`VimlrsRunConfigurationType`) | runs `viml FILE.vim` (positional file argument); toggle for `--disasm` (fusevm bytecode listing); working directory + script args + interpreter args |
| **Context menu** | *Run with vimlrs* on any `.vim` file in the editor or project view; auto-creates a config |
| **Producer** | `VimlrsRunConfigurationProducer` materializes a run config from the active file |
| **Output** | Standard `ConsoleView` — `echo` / `echomsg` stream in real time |
| **File → New → VimL File** | Pick *Script* (shebanged), *Autoload*, *Ftplugin*, or *Empty* |

### Debug

DAP-backed, over the `viml --dap` server's stdio. The plugin spawns `viml --dap`; the protocol frames flow over the process's stdout/stdin while the debuggee's own output arrives as DAP `output` events.

| Feature | Notes |
|---------|-------|
| Line breakpoints | Gutter toggle / enable / disable; persistent across sessions |
| Continue / Step Over / Step Into / Step Out / Pause / Run to Cursor | Standard XDebugger actions |
| Frames | `file:line` per frame, click to navigate source |
| Variables panel | Scalars, lists, dictionaries; expandable on click |
| Evaluate dialog | Arbitrary VimL expressions resolved against the paused frame |
| Console | `echo` / `echomsg` streams in real time via DAP `output` events |

---

## [0x06] DAP PROTOCOL

Plugin side (`com.menketechnologies.vimlrs.dap`):

1. `VimlrsDebugRunner.doExecute` spawns `viml --dap` and keeps its stdio for the DAP protocol.
2. `VimlrsDapClient` reads Content-Length-framed JSON-RPC from the process stdout — **byte-based, not char-based** — so multi-byte UTF-8 in variable reprs doesn't desync framing.
3. On `stopped` event, `onStopped` synchronously fetches `stackTrace` + `scopes` + `variables`, builds `VimlrsStackFrame` objects with pre-populated children, then calls `session.positionReached`.
4. `VimlrsEvaluator` sends `evaluate` requests for the Evaluate dialog.

vimlrs side (`src/dap.rs`): DAP requests handled include `initialize`, `launch`, `setBreakpoints`, `configurationDone`, `threads`, `stackTrace`, `scopes`, `variables`, `continue`, `next`, `stepIn`, `stepOut`, `pause`, `evaluate`, `disconnect`. Same JSON-RPC framing as the LSP server.

---

## [0x07] REFACTOR / RENAME

**Shift-F6** is handled by `VimlrsRenameHandler.kt`, which sends `textDocument/rename` to the LSP server. `src/lsp.rs` does not implement `textDocument/rename`, so the request is answered with `MethodNotFound`.

---

## [0x08] CONFIGURATION

*Settings → Tools → Vimlrs*:

| Section     | Setting                                | Default              | Notes |
|-------------|----------------------------------------|----------------------|-------|
| Interpreter | vimlrs executable                      | first `viml` on `$PATH` | absolute path or blank |
| LSP         | Enable LSP                             | on                   | master toggle |
| LSP         | Extra LSP args                         | empty                | passed after `--lsp` |
| LSP         | LSP environment                        | empty                | `KEY=VAL` pairs (e.g. `RUST_LOG=info`) |
| LSP         | Auto-restart LSP on settings change    | on                   | restart picks up new env |
| LSP         | Show builtin hovers                    | on                   | server-provided cards |
| LSP         | Log LSP traffic to file                | off                  | sets `VIMLRS_LSP_LOG=<path>` in the server environment (the server does not read it) |
| Editor      | Disable lexer highlighting             | off                  | turns the hand-rolled lexer highlighting off |
| Editor      | File extensions                        | `vim`                | comma-separated; the vimrc dotfiles always match |

Color scheme entries: *Settings → Editor → Color Scheme → vimlrs*.

---

## [0x09] LOGS

The plugin writes an append-only log under `~/.vimlrs/` (or `$VIMLRS_HOME/` when that env var is set):

| File | Source | Contents |
|------|--------|----------|
| `~/.vimlrs/vimlrs-plugin.log` | Kotlin (plugin) | LSP command line built, DAP `send` / receive, rename routing, breakpoint handler steps |

Tail with `tail -f ~/.vimlrs/vimlrs-plugin.log`.

---

## [0x0A] BUILDING

```sh
cd editors/intellij
export JAVA_HOME=$(/usr/libexec/java_home -v 17)   # macOS; or set to any JDK 17 install
./gradlew buildPlugin             # → build/distributions/vimlrs-intellij-<v>.zip
./gradlew runIde                  # launches a sandbox IDE with the plugin installed
./gradlew verifyPlugin            # plugin verifier against recommended IDE matrix
./gradlew test                    # runs VimlrsLexerTest + VimlrsCommenterTest + VimlrsSettingsTest + VimlrsSmartEnterProcessorTest
```

**JDK 17 is required.** Set `JAVA_HOME` to a JDK 17 install before running gradle. The plugin itself targets JVM 17, so any IDE on 2025.2+ runs it. First build downloads the IntelliJ Platform SDK (~1 GB), takes a few minutes, and is cached under `editors/intellij/.intellijPlatform/` (which is gitignored).

---

## [0x0B] PLUGIN ARCHITECTURE

```
editors/intellij/
├── build.gradle.kts                          # IntelliJ Platform Gradle Plugin 2.16
├── gradle.properties                         # platform version, plugin version, JVM
├── settings.gradle.kts
└── src/main/
    ├── kotlin/com/menketechnologies/vimlrs/
    │   ├── VimlrsLanguage.kt                 # Language singleton
    │   ├── VimlrsFileType.kt                 # .vim + vimrc family → VimL
    │   ├── VimlrsIcons.kt                    # icon loader
    │   ├── VimlrsColors.kt                   # VIMLRS_* TextAttributesKey constants
    │   ├── VimlrsTokenTypes.kt               # token type set
    │   ├── VimlrsLexer.kt                    # hand-rolled VimL lexer
    │   ├── VimlrsSyntaxHighlighter.kt        # token → color mapping
    │   ├── VimlrsColorSettingsPage.kt        # IDE color-scheme entries
    │   ├── VimlrsBraceMatcher.kt             # {} () []
    │   ├── VimlrsCommenter.kt                # `"` line comments
    │   ├── VimlrsQuoteHandler.kt             # " ' auto-pair
    │   ├── VimlrsSmartEnterProcessor.kt      # block / bracket completion
    │   ├── VimlrsTypedHandler.kt             # skip-over for closing brackets
    │   ├── VimlrsParserDefinition.kt         # flat parser definition over the lexer
    │   ├── VimlrsSpellcheckingStrategy.kt    # suppress typos on strings/comments
    │   ├── VimlrsSettings.kt                 # persistent settings
    │   ├── VimlrsSettingsConfigurable.kt
    │   ├── VimlrsDebugLog.kt                 # plugin-side log writer
    │   ├── lsp/
    │   │   ├── VimlrsLspServerSupportProvider.kt
    │   │   └── VimlrsLspServerDescriptor.kt
    │   ├── refactor/
    │   │   ├── VimlrsRefactoringSupportProvider.kt
    │   │   └── VimlrsRenameHandler.kt
    │   ├── navigate/
    │   │   └── VimlrsGotoDeclarationHandler.kt
    │   ├── run/
    │   │   ├── VimlrsRunConfigurationType.kt
    │   │   ├── VimlrsRunConfigurationOptions.kt
    │   │   ├── VimlrsRunConfiguration.kt
    │   │   ├── VimlrsRunConfigurationEditor.kt
    │   │   ├── VimlrsRunConfigurationProducer.kt
    │   │   ├── VimlrsProgramRunner.kt        # Run executor
    │   │   └── VimlrsDebugRunner.kt          # Debug executor (DAP over stdio)
    │   ├── dap/
    │   │   ├── VimlrsDapClient.kt            # byte-based DAP protocol client
    │   │   ├── VimlrsDebugProcess.kt         # XDebugProcess
    │   │   ├── VimlrsDebuggerEditorsProvider.kt
    │   │   ├── VimlrsBreakpointType.kt
    │   │   ├── VimlrsBreakpointHandler.kt
    │   │   ├── VimlrsStackFrame.kt
    │   │   ├── VimlrsSuspendContext.kt
    │   │   ├── VimlrsValue.kt
    │   │   └── VimlrsEvaluator.kt
    │   └── actions/
    │       ├── RunVimlrsFileAction.kt
    │       └── CreateVimlrsFileAction.kt
    └── resources/
        ├── META-INF/plugin.xml
        └── icons/vimlrs.svg
```

The Rust side lives in `vimlrs/src/lsp.rs` (LSP server, `viml --lsp`) and `vimlrs/src/dap.rs` (DAP server, `viml --dap`).

---

## [0x0C] VERSION COMPATIBILITY

`gradle.properties` carries the plugin version (`pluginVersion`) and controls the supported IDE range via `pluginSinceBuild` / `pluginUntilBuild`. Currently targets the `2025.2` SDK against builds `252..261.*` — every paid JetBrains IDE on **2025.2 +** loads it (RustRover, IDEA Ultimate, GoLand, PyCharm Pro, WebStorm, RubyMine, PhpStorm, CLion, Rider, DataGrip, Aqua). Community editions don't have the LSP API, so the plugin won't load there.

---

## [0x0D] LIMITATIONS

- **No PSI tree** — the parser definition is flat (one leaf per lexer token). Symbol features route through the LSP server, which currently provides document symbols only; go-to-definition, Find Usages and rename are not implemented server-side.
- **Debugger v1**: no conditional breakpoints, no hit-count breakpoints, no exception breakpoints, no watch expressions, no Set Value, single-thread only.
- **Lexer is approximate** for the `"` comment-vs-string ambiguity in pathological cases (a `"` at command position is a comment; otherwise a string — Vim's own runtime syntax uses the same heuristic).

---

## [0xFF] LICENSE

MIT, same as vimlrs.
