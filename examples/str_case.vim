" str_case.vim — case folding, whitespace trimming, and regex match POSITION
" builtins (charset.c/regexp.c: toupper/tolower, trim with a mask + direction,
" match/matchend/matchlist/matchstr). toupper/tolower fold Latin-1 accents;
" trim strips a character set from one or both ends; match returns the BYTE
" start, matchend the byte just past the match, matchlist the whole+submatch
" list ([] on no match). Self-test: asserts into v:errors, throws if failed.

" --- toupper()/tolower(): fold ASCII and accented Latin letters, keep digits
call assert_equal('HÉLLO', toupper('héllo'))
call assert_equal('héllo', tolower('HÉLLO'))
call assert_equal('ABC123', toupper('abc123'))
call assert_equal('', toupper(''))

" --- trim(): default strips leading+trailing whitespace (multibyte body kept)
call assert_equal('hi', trim('  hi  '))
call assert_equal('héllo', trim('  héllo  '))
" with a mask string; direction 0=both, 1=leading, 2=trailing
call assert_equal('hi', trim('xxhixx', 'x'))
call assert_equal('hixx', trim('xxhixx', 'x', 1))
call assert_equal('xxhi', trim('xxhixx', 'x', 2))
call assert_equal('hi', trim('••hi••', '•'))
call assert_equal('', trim('---', '-'))

" --- match(): byte offset of the first match, optional start, -1 if none
call assert_equal(2, match('hello', 'l'))
call assert_equal(3, match('hello', 'l', 3))
call assert_equal(-1, match('hello', 'z'))
call assert_equal(4, match('abcabc', 'bc', 3))

" --- matchend(): byte offset just PAST the match (so end-start = match length)
call assert_equal(3, matchend('foobar', 'o\+'))
call assert_equal(6, matchend('abcabc', 'bc', 3))
call assert_equal(5, matchend('café', 'é'))
call assert_equal(-1, matchend('foobar', 'x'))

" --- matchstr(): the matched text itself, '' when nothing matches
call assert_equal('123', matchstr('foo123bar', '\d\+'))
call assert_equal('', matchstr('abc', '\d'))

" --- matchlist(): [whole, \1, \2, …] padded to 10, or [] on no match
call assert_equal(['a1', 'a', '1', '', '', '', '', '', '', ''], matchlist('a1b2', '\(\a\)\(\d\)'))
call assert_equal(['2023-01-15', '2023', '01', '15', '', '', '', '', '', ''], matchlist('2023-01-15', '\(\d\+\)-\(\d\+\)-\(\d\+\)'))
call assert_equal([], matchlist('abc', 'x'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'str_case.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'str_case.vim: all assertions passed'
