vim9script
# vim9_def.vim — vim9script foundation: the `vim9script` marker, `def … enddef`
# functions with bare (a:-less) parameters, `: type` annotations, and vim9
# automatic line continuation (`:help vim9-line-continuation`).
#
# Everything below is real vim9script, binary-verified against Vim 9.2. Types
# are parsed and ignored (the vim9 type system is deferred); the runtime values
# are what the assertions pin. Self-tests into v:errors.
#
#   vimlrs examples/vim9_def.vim

# ── no-argument def with a return type ──
def Answer(): number
  return 42
enddef
assert_equal(42, Answer())

# ── bare parameter access: `x`/`y` refer to the params directly, no `a:` ──
def Add(x: number, y: number): number
  return x + y
enddef
assert_equal(5, Add(2, 3))

# ── an optional parameter with a default value ──
def Greet(name: string, punct: string = '!'): string
  return 'hi ' .. name .. punct
enddef
assert_equal('hi ada!', Greet('ada'))
assert_equal('hi ada.', Greet('ada', '.'))

# ── vim9 continuation: unclosed [] spans lines (the bracket-auto-continuation) ──
def MakeList(): list<number>
  return [
    1,
    2,
    # a comment line inside the list is dropped, the list keeps going
    3,
    ]
enddef
assert_equal([1, 2, 3], MakeList())

# ── vim9 continuation: a call whose arguments span multiple lines ──
def Sum3(a: number, b: number, c: number): number
  return a + b + c
enddef
def CallSpanning(): number
  return Sum3(
    10,
    20,
    30)
enddef
assert_equal(60, CallSpanning())

# ── vim9 continuation: leading binary operator (`+`) and concat (`..`) ──
def LeadingOps(): number
  return 1
    + 2
    + 3
enddef
assert_equal(6, LeadingOps())

def Concat(): string
  var a = 'x'
  return a
    .. 'y'
    .. 'z'
enddef
assert_equal('xyz', Concat())

# ── vim9 continuation: trailing binary operator (line ends with `+`) ──
def TrailingOp(): number
  return 100 +
    23
enddef
assert_equal(123, TrailingOp())

# ── vim9 continuation: a leading-`?`/`:` ternary across three lines ──
def Sign(n: number): string
  return n > 0
    ? 'pos'
    : 'neg'
enddef
assert_equal('pos', Sign(5))
assert_equal('neg', Sign(-5))

# ── recursion through a def (params bound per call) ──
def Fact(n: number): number
  if n <= 1
    return 1
  endif
  return n * Fact(n - 1)
enddef
assert_equal(120, Fact(5))

# ── `var` declaration with a `: type` annotation (type parsed, ignored) ──
def TypedLocals(): number
  var a: number = 10
  var b = 20
  return a + b
enddef
assert_equal(30, TypedLocals())

# ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) .. ' assertion(s) failed'
endif
