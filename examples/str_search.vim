" str_search.vim — the byte-offset string search builtins: stridx/strridx
" (literal substring, first/last), match/matchend (regex start/end byte offset),
" matchstr (matched text, with an optional {start}) and matchstrpos (text plus
" [start, end] offsets). All offsets are BYTE positions; a miss is -1 (and the
" empty '', [-1, -1] shapes for the *str/*pos forms). Self-test: asserts into
" v:errors, throws if any failed.

" --- stridx(): first byte offset of a literal substring, -1 if absent
call assert_equal(2, stridx('mississippi', 'ss'))
call assert_equal(-1, stridx('abc', 'z'))
call assert_equal(-1, stridx('', 'x'))
call assert_equal(0, stridx('hello', ''))

" --- stridx() {start}: begin searching at a byte offset
call assert_equal(7, stridx('hello world', 'o', 5))
call assert_equal(-1, stridx('abc', 'b', 2))

" --- strridx(): LAST occurrence of a literal substring
call assert_equal(5, strridx('mississippi', 'ss'))
call assert_equal(4, strridx('abcabc', 'bc'))
call assert_equal(7, strridx('hello world', 'o'))

" --- match(): byte offset where a regex first matches, -1 on no match
call assert_equal(2, match('hello', 'l'))
call assert_equal(-1, match('foobar', '\d'))

" --- match() {start}: resume the search at a byte offset
call assert_equal(3, match('hello', 'l', 3))
call assert_equal(3, match('aXbXc', 'X', 2))

" --- matchend(): byte offset just PAST the match (start+len), -1 on miss
call assert_equal(4, matchend('a123', '\d\+'))
call assert_equal(9, matchend('foobar123', '\d\+'))
call assert_equal(-1, matchend('nomatch', '\d'))

" --- matchstr(): the matched text (not an offset); '' when nothing matches
call assert_equal('123', matchstr('foobar123', '\d\+'))
call assert_equal('', matchstr('foobar', '\d\+'))

" --- matchstr() {start}: search from a byte offset (skips the first run)
call assert_equal('22', matchstr('a1b22c', '\d\+', 3))

" --- matchstrpos(): [text, start, end] byte offsets; ['', -1, -1] on no match
call assert_equal(['9', 2, 3], matchstrpos('xx9', '\d'))
call assert_equal(['1', 1, 2], matchstrpos('a1b2', '\d'))
call assert_equal(['', -1, -1], matchstrpos('nope', '\d'))

" --- matchlist(): whole match followed by the nine \1..\9 submatch slots
call assert_equal(['2024-01', '2024', '01', '', '', '', '', '', '', ''], matchlist('2024-01', '\(\d\+\)-\(\d\+\)'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'str_search.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'str_search.vim: all assertions passed'
