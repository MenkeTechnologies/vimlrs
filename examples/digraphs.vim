" digraphs.vim — digraph_get()/digraph_set()/digraph_getlist()/
" digraph_setlist(), the digraph table ported from Neovim's digraph.c.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- the built-in RFC-1345 subset resolves common two-char triggers
call assert_equal('ä', digraph_get('a:'))
call assert_equal('ö', digraph_get('o:'))
call assert_equal('©', digraph_get('Co'))
call assert_equal('→', digraph_get('->'))

" --- an unknown trigger yields an empty string
call assert_equal('', digraph_get('zz'))

" --- digraph_set() registers a user digraph and returns v:true
call assert_equal(v:true, digraph_set('(c', 'X'))
call assert_equal('X', digraph_get('(c'))

" --- a user digraph overrides the built-in for the same trigger
call assert_equal(v:true, digraph_set('a:', 'A'))
call assert_equal('A', digraph_get('a:'))

" --- digraph_setlist() registers several at once; digraph_getlist() lists the
"     user digraphs as [chars, digraph] pairs, sorted by trigger
call assert_equal(v:true, digraph_setlist([['e=', 'Y'], ['o=', 'Z']]))
call assert_equal('Y', digraph_get('e='))
call assert_equal('Z', digraph_get('o='))
call assert_equal([['(c', 'X'], ['a:', 'A'], ['e=', 'Y'], ['o=', 'Z']], digraph_getlist())

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'digraphs.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'digraphs.vim: all assertions passed'
