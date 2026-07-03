" strwidth_funcs.vim — length/width and codepoint<->character conversion
" (mbyte.c/charset.c: strlen/strchars/strwidth/strdisplaywidth, nr2char/char2nr).
" strlen() counts BYTES (a multibyte char is several); strchars() counts
" CHARACTERS (codepoints); strwidth()/strdisplaywidth() count display CELLS.
" nr2char() maps a codepoint number to its character, char2nr() the inverse of
" the first character. Self-test: asserts into v:errors, throws if any failed.

" --- strlen(): BYTE count (é = 2 bytes, 😀 = 4 bytes)
call assert_equal(5, strlen('hello'))
call assert_equal(6, strlen('héllo'))
call assert_equal(4, strlen('😀'))
call assert_equal(0, strlen(''))

" --- strchars(): CHARACTER (codepoint) count
call assert_equal(5, strchars('héllo'))
call assert_equal(3, strchars('😀ab'))
call assert_equal(0, strchars(''))

" --- strwidth()/strdisplaywidth(): display CELLS (plain text == character count)
call assert_equal(5, strwidth('hello'))
call assert_equal(5, strwidth('héllo'))
call assert_equal(0, strwidth(''))
call assert_equal(5, strdisplaywidth('hello'))
call assert_equal(5, strdisplaywidth('héllo'))

" --- nr2char(): codepoint number -> character (ASCII, Latin-1, astral emoji)
call assert_equal('A', nr2char(65))
call assert_equal('a', nr2char(97))
call assert_equal('é', nr2char(233))
call assert_equal('😀', nr2char(0x1F600))

" --- char2nr(): first character -> codepoint number (0 for empty)
call assert_equal(65, char2nr('A'))
call assert_equal(233, char2nr('é'))
call assert_equal(128512, char2nr('😀'))
call assert_equal(0, char2nr(''))

" --- round-trip: nr2char(char2nr(c)) == c for each kind
call assert_equal('é', nr2char(char2nr('é')))
call assert_equal('😀', nr2char(char2nr('😀')))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'strwidth_funcs.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'strwidth_funcs.vim: all assertions passed'
