" strindex.vim — byte/char index + character-extraction string functions
" (funcs.c: byteidx/byteidxcomp/charidx, strgetchar/strcharpart, char2nr),
" plus values()/shellescape()/matchstrpos(). Uses a multibyte string ('héllo',
" where 'é' is two UTF-8 bytes) so the byte-vs-char distinction is exercised.
" Self-test: asserts into v:errors, throws if any failed.

" --- byteidx: byte offset of the Nth character; 'é' spans bytes 1..2
call assert_equal(0, byteidx('héllo', 0))
call assert_equal(1, byteidx('héllo', 1))
call assert_equal(3, byteidx('héllo', 2))
call assert_equal(3, byteidxcomp('héllo', 2))

" --- charidx is the inverse: character index at a byte offset
call assert_equal(0, charidx('héllo', 0))
call assert_equal(2, charidx('héllo', 3))

" --- strgetchar returns the codepoint at a char index; char2nr a 1-char string
call assert_equal(char2nr('a'), strgetchar('abc', 0))
call assert_equal(98, strgetchar('abc', 1))
call assert_equal(65, char2nr('A'))

" --- strcharpart slices by character (multibyte-aware), not byte
call assert_equal('ell', strcharpart('hello', 1, 3))
call assert_equal('hé', strcharpart('héllo', 0, 2))

" --- values: the dict's values (order-independent, so compare sorted)
call assert_equal([1, 2], sort(values({'a': 1, 'b': 2})))

" --- shellescape single-quotes the argument and escapes embedded quotes
call assert_equal("'a b'", shellescape('a b'))
call assert_equal("'it'\\''s'", shellescape("it's"))

" --- matchstrpos returns [matched-text, start, end] in char positions
call assert_equal(['oo', 1, 3], matchstrpos('foobar', 'o\+'))
call assert_equal(['', -1, -1], matchstrpos('foobar', 'z\+'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'strindex.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'strindex.vim: all assertions passed'
