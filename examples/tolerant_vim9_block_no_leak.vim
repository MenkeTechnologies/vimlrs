vim9script
# tolerant_vim9_block_no_leak.vim — the vim9 definition blocks `:enum`/`:class`/
# `:interface` and an `export def` whose body vimlrs cannot yet parse are each
# consumed WHOLE by the error-tolerant sourcer, never leaking inner statements.
#
# Regression: the tolerant sourcer classifies block openers to skip a broken
# body as one unit. It must see through the vim9 `export` modifier (the raw
# command word of `export def …` is `export`, not `def`) and must recognise the
# vim9 definition keywords `enum`/`class`/`interface` — otherwise the block's
# body leaks out and a leaked loop hard-crashes under the block JIT. Self-test
# into v:errors.

# --- An `enum` block whose body would, if leaked, run `Prompt,`/`Terminal` as
#     top-level expressions. It is skipped whole instead.
enum Way
  Prompt,
  Terminal
endenum

# --- An `export def` (raw command word `export`) with an unsupported curly-brace
#     name in a `while` loop. If its body leaked, `g:leaked_from_def` would be set
#     at the top level; it must stay undefined.
export def BrokenBody(): void
  g:leaked_from_def = 1
  var pos = 0
  while pos != -1
    var x = open_{pos}
    pos = -1
  endwhile
enddef

assert_false(exists('g:leaked_from_def'))

# --- A top-level statement after the skipped blocks still takes effect.
g:after_blocks = 7
assert_equal(7, g:after_blocks)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'tolerant_vim9_block_no_leak.vim: ' .. len(v:errors) .. ' assertion(s) failed'
endif
echo 'tolerant_vim9_block_no_leak.vim: all assertions passed'
