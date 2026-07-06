vim9script
# vim9_script_scope.vim — vim9 script-scope variables are visible inside `def`
# bodies by bare name (`:help vim9-scopes`). A `def` sees script-level `var` and
# `const` declarations without an `s:`/`g:` prefix; a legacy `:function` does
# not. Binary-verified against Vim 9.2. Self-tests into v:errors.
#
#   vimlrs examples/vim9_script_scope.vim

# ── a script-level `var` is readable inside a def by bare name ──
var greeting = 'hello'
def Read(): string
  return greeting
enddef
assert_equal('hello', Read())

# ── a script-level `const` is readable inside a def by bare name ──
const LIMIT = 42
def Limit(): number
  return LIMIT
enddef
assert_equal(42, Limit())

# ── a script var read inside an `if` condition in a def body ──
var threshold = 10
def Below(n: number): bool
  if n < threshold
    return true
  endif
  return false
enddef
assert_equal(true, Below(3))
assert_equal(false, Below(20))

# ── reassigning a script var inside an `if` in a def mutates the script var ──
var counter = 0
def Bump()
  if counter < 10
    counter = counter + 1
  endif
enddef
Bump()
Bump()
Bump()
assert_equal(3, counter)

# ── a script var used in arithmetic with a def-local ──
var base = 100
def AddBase(n: number): number
  var extra = 5
  return base + n + extra
enddef
assert_equal(115, AddBase(10))

# ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) .. ' assertion(s) failed'
endif
