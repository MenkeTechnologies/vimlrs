vim9script
# vim9_var_assign.vim — vim9script variable declaration and assignment:
#   • a type-only `var x: T` declaration (no initializer) default-inits to T's
#     zero value (`:help vim9-declaration`);
#   • a bare `x = expr` / `x += expr` / `x ..= expr` reassignment (no `:let`/
#     `:var` keyword), the vim9 way of assigning to a declared variable.
#
# Everything below is real vim9script, binary-verified against Vim 9.2. Types
# are parsed and ignored (the type system is deferred); the runtime values are
# what the assertions pin. Self-tests into v:errors.
#
#   vimlrs examples/vim9_var_assign.vim

# ── type-only declaration → per-type zero value (string '', number 0, list [],
#    dict {}, bool false, float 0.0) ──
def Defaults(): list<any>
  var s: string
  var n: number
  var l: list<number>
  var d: dict<number>
  var b: bool
  var f: float
  return [s, n, l, d, b, f]
enddef
var r = Defaults()
assert_equal('', r[0])
assert_equal(0, r[1])
assert_equal([], r[2])
assert_equal({}, r[3])
assert_equal(false, r[4])
assert_equal(0.0, r[5])

# ── bare reassignment: compound arithmetic (`+=`, `*=`), string append (`..=`),
#    indexed dict assign, and method-call mutation of a default-init list ──
def Reassign(): list<any>
  var n = 5
  n += 3
  n *= 2
  var s = 'a'
  s ..= 'b'
  s ..= 'c'
  var d: dict<number>
  d['k'] = 9
  var lst: list<number>
  lst->add(1)
  lst->add(2)
  return [n, s, d, lst]
enddef
var q = Reassign()
assert_equal(16, q[0])
assert_equal('abc', q[1])
assert_equal({'k': 9}, q[2])
assert_equal([1, 2], q[3])

# ── top-level bare reassignment (outside any def) ──
var top = 10
top -= 4
assert_equal(6, top)

# ── a CamelCase script variable assigns (must not be read as a user command) ──
var Total = 0
Total += 7
assert_equal(7, Total)

# ── bare list-unpack assignment `[a, b] = expr` (no `var`/`:let` keyword): the
#    vim9 way to reassign several already-declared names at once. Distinct from
#    the `var [a, b] = …` declaration form; binary-verified against Vim 9.2. ──
var ua = 0
var ub = 0
[ua, ub] = [1, 2]
assert_equal(1, ua)
assert_equal(2, ub)

# swap: the rhs is fully built before any target is bound
[ua, ub] = [ub, ua]
assert_equal([2, 1], [ua, ub])

# `[a, b; rest]`: the trailing name soaks up the remainder as a list
var uh = 0
var ut = 0
var urest: list<number> = []
[uh, ut; urest] = [10, 20, 30, 40]
assert_equal(10, uh)
assert_equal(20, ut)
assert_equal([30, 40], urest)

# a rest binding matches exactly zero remaining elements
[uh; urest] = [99]
assert_equal(99, uh)
assert_equal([], urest)

# unpack assignment inside a def, targeting locals
def UnpackLocal(): number
  var x = 0
  var y = 0
  [x, y] = [3, 4]
  return x + y
enddef
assert_equal(7, UnpackLocal())

# ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) .. ' assertion(s) failed'
endif
