" match_startcount.vim — the {start} and {count} arguments of the match family
" (funcs.c find_some_match). match/matchend/matchstr/matchstrpos/matchlist all
" accept an optional {start} (where to begin) and {count} (which match: the Nth).
" Self-test into v:errors.

" --- match()/matchend() with {start}
call assert_equal(1, match('abcabc', 'b'))
call assert_equal(4, match('abcabc', 'b', 3))
call assert_equal(5, matchend('abcabc', 'b', 3))

" --- {count}: the Nth match (1-based), counting from {start}
call assert_equal(4, match('abcabc', 'b', 0, 2))
call assert_equal('Y', matchstr('aXbYc', '[A-Z]', 0, 2))
call assert_equal(-1, match('abcabc', 'b', 0, 3))

" --- matchstr()/matchlist() honour {start}
call assert_equal('2', matchstr('a1b2', '\d', 2))
call assert_equal('2', matchlist('a1b2', '\d', 2)[0])

" --- matchstrpos(): [match, start, end], shifted by {start}
call assert_equal(['2', 3, 4], matchstrpos('a1b2', '\d', 2))
call assert_equal(['', -1, -1], matchstrpos('abc', '\d'))

" --- regressions: no {start}/{count} behaves as before
call assert_equal(1, match('foobar', 'o'))
call assert_equal('oo', matchstr('foobar', 'o\+'))
call assert_equal(3, matchend('foobar', 'o\+'))
call assert_equal(['1', '', '', '', '', '', '', '', '', ''], matchlist('a1', '\d'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'match_startcount.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'match_startcount.vim: all assertions passed'
