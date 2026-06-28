" search.vim — buffer search: search()/searchpos() move the cursor to a pattern,
" searchpair()/searchpairpos() find a matching nested pair, searchcount() counts
" matches of the last pattern (ported from Neovim's search.c). These build on the
" in-memory buffer + cursor + the Vim regex engine, so they run standalone.
" Self-test: asserts into v:errors, throws at the end if anything failed.

call setline(1, ['foo bar', 'baz foo', 'qux', 'foo end'])

" --- search() moves to the next match and returns its line; a match at the
"     cursor is skipped unless the 'c' flag is given
call cursor(1, 1)
call assert_equal(2, search('foo'))
call assert_equal([0, 2, 5, 0], getpos('.'))
call assert_equal(4, search('foo'))

" --- the 'c' flag accepts a match at the cursor
call cursor(1, 1)
call assert_equal(1, search('foo', 'c'))

" --- the 'n' flag finds without moving the cursor
call cursor(1, 1)
call assert_equal(2, search('baz', 'n'))
call assert_equal(1, line('.'))

" --- the 'b' flag searches backward
call cursor(4, 1)
call assert_equal(2, search('foo', 'b'))

" --- the 'e' flag moves to the end of the match
call cursor(1, 1)
call assert_equal(1, search('bar', 'e'))
call assert_equal(7, col('.'))

" --- searchpos() returns [lnum, col]; not found is [0, 0]
call cursor(1, 1)
call assert_equal([1, 5], searchpos('bar'))
call assert_equal([0, 0], searchpos('nope', 'n'))

" --- searchpair() finds the matching end of a nested pair
call setline(1, ['if x', '  if y', '  endif', 'endif'])
call deletebufline('', 5, '$')
call cursor(1, 1)
call assert_equal(4, searchpair('\<if\>', '', '\<endif\>'))

call setline(1, ['( a ( b ) c )', 'd'])
call deletebufline('', 3, '$')
call cursor(1, 1)
call assert_equal([1, 13], searchpairpos('(', '', ')'))

" --- searchcount() reports the position among matches of the last pattern
call setline(1, ['foo a foo', 'b foo c', 'd'])
call deletebufline('', 4, '$')
call cursor(1, 1)
call search('foo')
let sc = searchcount()
call assert_equal(3, sc.total)
call assert_equal(2, sc.current)
call assert_equal(1, sc.exact_match)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'search.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'search.vim: all assertions passed'
