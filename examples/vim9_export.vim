vim9script
# vim9_export.vim — the vim9 `export` modifier on `def`/`var`/`const`, with
# embedded unit tests. `export` marks a definition visible to importers
# (`:help :export`); editor-less the marker has no runtime effect, so an
# `export def`/`export var`/`export const` behaves exactly like the un-exported
# form. The definition still registers: before this was handled, an
# `export def` body ran as top-level statements instead of defining a function.
#
# Real vim9script, binary-verified against Vim 9.2. Self-tests into v:errors.
#
#   vimlrs examples/vim9_export.vim

# ── exported constant and variable are ordinary script values ──
export const LIMIT = 100
export var greeting = 'hi'
assert_equal(100, LIMIT)
assert_equal('hi', greeting)

# ── an exported def registers and can forward-reference a later def ──
export def Compute(n: number): number
  return Double(n) + 1
enddef

def Double(n: number): number
  return n * 2
enddef

assert_equal(21, Compute(10))
assert_equal(41, Compute(20))

# ── exported def with a default parameter value ──
export def Greet(name: string, punct: string = '!'): string
  return 'hello ' .. name .. punct
enddef
assert_equal('hello world!', Greet('world'))
assert_equal('hello world?', Greet('world', '?'))

# ── an exported def whose body uses an arrow lambda ──
export def Doubled(items: list<number>): list<number>
  return items->mapnew((_, v) => v * 2)
enddef
assert_equal([2, 4, 6], Doubled([1, 2, 3]))

# ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) .. ' assertion(s) failed'
endif
echo 'OK: vim9_export assertions passed'
