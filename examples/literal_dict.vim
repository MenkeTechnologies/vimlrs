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

" ── a dict literal with more than 127 pairs still builds ONE dict ──
"    VIML_MAKE_DICT carries its slot count (2 per pair) in a u8, so a literal
"    caps at 127 pairs per op; a longer literal is built in 127-pair chunks and
"    merged with extend(). Vim puts no size limit on a dict literal (e.g.
"    colors/lists/default.vim's 788-entry v:colornames), so the result must be a
"    single dict of the full length with every key present and correct.
let big = {}
let big = eval('{' . join(map(range(300), '"''k" . v:val . "'': " . (v:val * 2)'), ', ') . '}')
call assert_equal(300, len(big))
call assert_equal(0, big.k0)
call assert_equal(254, big.k127)
call assert_equal(256, big.k128)
call assert_equal(598, big.k299)
call assert_true(has_key(big, 'k200'))

" ── demo ──
echo '#{foo: 1, bar: 2} ->' #{foo: 1, bar: 2}
echo 'nested tree       ->' tree

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'literal_dict.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
