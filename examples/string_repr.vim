" string_repr.vim — string() encoding of every value type (eval.c encode path).
"
" string() renders a value as reparseable Vimscript: strings single-quoted with
" '' escaping, numbers/floats bare, Lists in [...], Dicts in {...} with sorted
" string keys, and the special values v:true/v:false/v:null by name. This is the
" typval_tostring()/encode_tv2string() path. Self-tests with assert_*; exits
" non-zero on any failure.
"
"   vimlrs examples/string_repr.vim

" ── scalars ──
call assert_equal("'hi'", string('hi'))
call assert_equal('42', string(42))
call assert_equal('-7', string(-7))
call assert_equal('1.5', string(1.5))

" ── a single quote inside a string is doubled ──
call assert_equal("'it''s'", string("it's"))

" ── special constants render by name ──
call assert_equal('v:true', string(v:true))
call assert_equal('v:false', string(v:false))
call assert_equal('v:null', string(v:null))

" ── containers, with dict keys sorted ──
call assert_equal('[1, 2, 3]', string([1, 2, 3]))
call assert_equal("{'a': 1, 'b': 2}", string(#{b: 2, a: 1}))
call assert_equal('[]', string([]))
call assert_equal('{}', string({}))

" ── nesting composes ──
call assert_equal("['a', {'k': [1, 2]}]", string(['a', #{k: [1, 2]}]))

" ── string() round-trips through eval() for simple values ──
for v in [42, 'text', [1, 2, 3], #{x: 1}]
  call assert_equal(v, eval(string(v)))
endfor

" ── demo ──
echo 'string(#{b:2, a:1}) ->' string(#{b: 2, a: 1})
echo "string([1, 'two'])  ->" string([1, 'two'])

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'string_repr.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
