" regex_zs.vim — \zs / \ze match-bound atoms and zero-width split() (regexp).
" \zs sets where the matched text starts, \ze where it ends, so matchstr() and
" substitute() report only the bracketed part. split() on a zero-width pattern
" (the \zs "split into characters" idiom, or any pattern that can match empty)
" advances one character at a time. Self-test: asserts into v:errors.

" --- \zs / \ze in matchstr(): only the text after \zs / before \ze is returned
call assert_equal('l', matchstr('hello', 'l\zs.'))
call assert_equal('foo', matchstr('foobar', 'foo\zebar'))
call assert_equal('123', matchstr('abc123', '\d\+'))

" --- \zs in substitute(): the replaced span starts at \zs
call assert_equal('Xbc', substitute('abc', '\zs.', 'X', ''))
call assert_equal('helXo', substitute('hello', 'l\zsl', 'X', ''))

" --- split() on \zs splits into characters
call assert_equal(['h', 'e', 'l', 'l', 'o'], split('hello', '\zs'))

" --- split() on any zero-width-capable pattern advances one char per item
call assert_equal(['a', 'b'], split('ab', 'x*'))

" --- regular (non-zero-width) split still works, internal empties kept
call assert_equal(['a', 'b', 'c'], split('a1b2c', '\d'))
call assert_equal(['a', '', 'b'], split('a,,b', ','))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'regex_zs.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'regex_zs.vim: all assertions passed'
