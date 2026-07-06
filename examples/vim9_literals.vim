vim9script
# vim9_literals.vim — vim9script keyword literals `true`/`false`/`null`
# (`:help vim9-boolean`, `:help null`). In a `:vim9script` script or a
# `def…enddef` body, bare `true`/`false`/`null` are the boolean/special
# constants; they equal `v:true`/`v:false`/`v:null`. In legacy Vimscript they
# are ordinary undefined names (bare `true` → E121), so this is vim9-only.
#
# Every assertion is binary-verified against Vim 9.2 (`vim -es -u NONE
# --cmd vim9script`): all pass with v:errors empty. Self-tests into v:errors.
#
#   vimlrs examples/vim9_literals.vim

# ── the literals equal their v: counterparts ──
assert_equal(v:true, true)
assert_equal(v:false, false)
assert_equal(v:null, null)

# ── types: true/false are Bool (6), null is Special (7) ──
assert_equal(6, type(true))
assert_equal(6, type(false))
assert_equal(7, type(null))
assert_equal(type(v:true), type(true))
assert_equal(type(v:null), type(null))

# ── truthiness ──
assert_true(true)
assert_false(false)

# ── comparisons against the v: constants ──
assert_true(true == v:true)
assert_true(false == v:false)
assert_true(null == v:null)
assert_true(true == true)
assert_true(null == null)

# ── boolean operators over the literals ──
assert_true(!false)
assert_false(!true)
assert_true(true && true)
assert_false(true && false)
assert_true(true || false)

# ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) .. ' assertion(s) failed'
endif
