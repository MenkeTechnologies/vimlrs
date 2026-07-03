" strconvert.vim — string<->codepoint conversion and substring search
" (funcs.c: str2list/list2str, stridx/strridx). str2list yields Unicode
" codepoints (not bytes), so a multibyte character is one entry, and list2str
" is its exact inverse. stridx/strridx return BYTE offsets: the first and last
" match respectively, -1 when absent. Self-test: throws if any failed.

" --- str2list(): one entry per codepoint (multibyte folds to a single number)
call assert_equal([65, 66, 67], str2list('ABC'))
call assert_equal([104, 233, 108, 108, 111], str2list('héllo'))
call assert_equal([65], str2list('A'))
call assert_equal([], str2list(''))

" --- list2str(): inverse of str2list, including astral (emoji) codepoints
call assert_equal('Hi', list2str([72, 105]))
call assert_equal('é', list2str([233]))
call assert_equal('', list2str([]))
call assert_equal('😀', list2str([0x1F600]))

" --- round-trip: list2str(str2list(s)) == s for a multibyte string
let s = 'héllo😀'
call assert_equal(s, list2str(str2list(s)))

" --- stridx(): byte offset of the FIRST match (optional start), -1 if none
call assert_equal(4, stridx('hello world', 'o'))
call assert_equal(7, stridx('hello world', 'o', 5))
call assert_equal(-1, stridx('hello', 'z'))
call assert_equal(-1, stridx('', 'a'))
call assert_equal(3, stridx('aXbXc', 'X', 2))

" --- strridx(): byte offset of the LAST match, -1 if none
call assert_equal(7, strridx('hello world', 'o'))
call assert_equal(4, strridx('abcabc', 'bc'))
call assert_equal(-1, strridx('abc', 'x'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'strconvert.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'strconvert.vim: all assertions passed'
