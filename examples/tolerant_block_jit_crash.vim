vim9script
# tolerant_block_jit_crash.vim — crash-hunt regression.
#
# Reduced from runtime/syntax/2html.vim (Vim 9.2). That file calls
# `tohtml#GetUserSettings()` (an autoload fn not present standalone), so the
# strict whole-file parse fails and vimlrs drops to its tolerant fallback:
# every surviving top-level statement runs once in its own tiny one-shot chunk.
#
# The pathological shape below is two or more typed `var` declarations followed
# by a block the strict parser cannot read (an `enum`, which vim 9.2 accepts).
# That is what forces the tolerant path; the declarations then run as separate
# chunks. (It was an unmatched `if a` with no `:endif`; vimlrs now runs an
# unclosed conditional at the end of a sourced file and reports E171 as vim
# does, so that no longer reaches the tolerant path.) Each compiles
# to the same opcode sequence (the variable name lives in the data pool, not the
# opcodes), so fusevm's block-JIT cache — keyed on `(op_hash, slot_kinds_hash)` —
# collides across them. The second to run crossed the warm-up threshold and got
# whole-chunk block-compiled, jumping into native code carrying the script-var
# store's host-helper call that the block JIT could not resolve: a jump to a null
# pointer, i.e. SIGSEGV (exit 139).
#
# Fix: the tolerant per-statement loop now runs its one-shot chunks on the
# interpreter (JIT suppressed) — behaviorally identical, since block-JITing a
# statement that runs exactly once buys nothing. Real Vim runs the declarations
# and defines the enum; it never crashes. This script
# must merely SOURCE without crashing (exit 0, no `E<num>:` on stderr) — there is
# nothing to assert because tolerant sourcing gives each statement a fresh scope.
var a: number
var b: number
var c: number
var d: number
var e: number
enum Way
  Prompt,
  Terminal
endenum
