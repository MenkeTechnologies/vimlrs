" literal_dict.vim — #{} literal-key dictionaries (eval.c get_literal_key).
"
" In a #{} dictionary the keys are bare literals (no quoting and no expression
" evaluation), so #{foo: 1} is exactly {'foo': 1}. Keys are made of letters,
" digits and underscores; a leading digit is fine because the key is literal,
" not an identifier. Self-tests with assert_*; exits non-zero on failure.
"
"   vimlrs examples/literal_dict.vim

" ── #{} is sugar for a string-keyed dictionary ──
let d = #{foo: 1, bar: 2}
call assert_equal({'foo': 1, 'bar': 2}, d)
call assert_equal(1, d.foo)
call assert_equal(2, d['bar'])

" ── digit-leading and underscore keys are valid literal keys ──
let codes = #{200: 'ok', 404: 'missing', not_found: 1}
call assert_equal('ok', codes['200'])
call assert_equal('missing', codes['404'])
call assert_equal(1, codes.not_found)
call assert_true(has_key(codes, '200'))

" ── unlike {}, the keys are NOT evaluated as expressions ──
let foo = 'EXPANDED'
let lit = #{foo: 10}
let expr = {foo: 10}
call assert_equal(10, lit.foo)
call assert_equal(10, expr['EXPANDED'])
call assert_false(has_key(expr, 'foo'))

" ── empty and nested literal dicts ──
call assert_equal({}, #{})
let tree = #{name: 'root', kids: [#{name: 'a'}, #{name: 'b'}]}
call assert_equal('root', tree.name)
call assert_equal('a', tree.kids[0].name)
call assert_equal(['kids', 'name'], sort(keys(tree)))

" ── demo ──
echo '#{foo: 1, bar: 2} ->' #{foo: 1, bar: 2}
echo 'nested tree       ->' tree

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'literal_dict.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
