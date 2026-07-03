" substr_funcs.vim — substring extraction by BYTE vs CHARACTER index
" (strings.c: strpart/strcharpart/strgetchar). strpart() indexes and counts in
" BYTES; strcharpart() indexes and counts in CHARACTERS (codepoints), so a
" multibyte body is sliced whole; strgetchar() returns the codepoint NUMBER at a
" character index (-1 past the end). Out-of-range starts clamp, over-long counts
" clamp to the tail. Self-test: asserts into v:errors, throws if any failed.

" --- strpart(): byte-indexed slice; ASCII only here so bytes == characters
call assert_equal('ell', strpart('hello', 1, 3))
call assert_equal('llo', strpart('hello', 2))
call assert_equal('he', strpart('hello', -2, 4))
call assert_equal('', strpart('hello', 10, 3))
call assert_equal('hello', strpart('hello', 0, 99))
call assert_equal('', strpart('hello', 3, 0))

" --- strcharpart(): character-indexed slice; multibyte body stays intact
call assert_equal('cde', strcharpart('abcdef', 2, 3))
call assert_equal('él', strcharpart('héllo', 1, 2))
call assert_equal('hél', strcharpart('héllo', 0, 3))
call assert_equal('😀', strcharpart('😀ab', 0, 1))
call assert_equal('a', strcharpart('abc', -1, 2))
call assert_equal('', strcharpart('abc', 5, 2))

" --- strgetchar(): codepoint at a CHARACTER index, -1 when past the end
call assert_equal(233, strgetchar('héllo', 1))
call assert_equal(104, strgetchar('héllo', 0))
call assert_equal(128512, strgetchar('😀', 0))
call assert_equal(-1, strgetchar('abc', 5))
call assert_equal(-1, strgetchar('', 0))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'substr_funcs.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'substr_funcs.vim: all assertions passed'
