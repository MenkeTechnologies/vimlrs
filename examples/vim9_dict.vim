vim9script
# vim9_dict.vim — vim9 bare-key dict literals. In vim9script (and in any
# `def … enddef` body) `{key: value}` uses BARE literal keys: `{a: 1}` has the
# string key "a", NOT the value of a variable `a` (that is the legacy form). See
# vim9.txt: "the {} form uses literal keys … for alphanumeric characters,
# underscore and dash". Quoted keys and `[expr]` computed keys still work.
#
# Every expected value below is binary-verified against Vim 9.2
# (`string()` of the same literal). assert_equal on a Dictionary compares
# content, not key order. Self-tests into v:errors.
#
#   vimlrs examples/vim9_dict.vim

# ── bare keys become string keys ──
assert_equal({'a': 1, 'b': 2}, {a: 1, b: 2})

# ── a bare key that looks like a number stringifies (leading zeros kept) ──
assert_equal({'1': 'x'}, {1: 'x'})
assert_equal({'007': 1}, {007: 1})

# ── underscore and dash are valid bare-key characters ──
assert_equal({'my_key': 5}, {my_key: 5})
assert_equal({'a-b': 1}, {a-b: 1})

# ── nested bare-key dicts ──
assert_equal({'x': {'y': 3}}, {x: {y: 3}})

# ── mixing bare and quoted keys in one literal ──
assert_equal({'a': 1, 'b': 2}, {a: 1, 'b': 2})

# ── quoted keys keep working unchanged ──
assert_equal({'k': 9}, {'k': 9})
assert_equal({'k': 9}, {"k": 9})

# ── the empty dict is a dict, not a lambda ──
assert_equal({}, {})

# ── [expr] computed keys: the expression is evaluated, then stringified ──
var key = 'z'
assert_equal({'z': 1}, {[key]: 1})
assert_equal({'3': 'v'}, {[1 + 2]: 'v'})

# ── values are ordinary expressions (functions, arithmetic, nesting) ──
assert_equal({'n': 6}, {n: 2 * 3})

# ── a def body is vim9 even when reached through a call ──
def MakeDict(): dict<number>
  return {one: 1, two: 2}
enddef
assert_equal({'one': 1, 'two': 2}, MakeDict())

# ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) .. ' assertion(s) failed'
endif
