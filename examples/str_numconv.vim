" str_numconv.vim — string<->number conversion and trimming (funcs.c/charset:
" str2nr parses an integer in a given base, str2float parses a float, char2nr /
" nr2char convert between a character and its Unicode codepoint, trim strips a
" leading/trailing character set, escape backslash-quotes chosen characters).
" str2nr accepts the 0x/0b/0-style prefixes matching its base and tolerates a
" leading sign or spaces. Self-test: asserts into v:errors, throws if any fail.

" --- str2nr(): base 10 default, plus explicit bases with optional 0x/0b prefix
call assert_equal(-42, str2nr('-42'))
call assert_equal(17, str2nr('  17'))
call assert_equal(0, str2nr(''))
call assert_equal(255, str2nr('ff', 16))
call assert_equal(26, str2nr('0x1A', 16))
call assert_equal(-255, str2nr('-ff', 16))
call assert_equal(511, str2nr('777', 8))
call assert_equal(5, str2nr('101', 2))
call assert_equal(5, str2nr('0b101', 2))
call assert_equal(0, str2nr('z', 16))

" --- str2float(): decimal, scientific, and integer-looking input
call assert_equal(3.14, str2float('3.14'))
call assert_equal(-50.0, str2float('-0.5e2'))
call assert_equal(42.0, str2float('42'))
call assert_equal(v:t_float, type(str2float('1')))

" --- char2nr() / nr2char(): codepoint round-trip, incl. multibyte
call assert_equal(65, char2nr('A'))
call assert_equal(233, char2nr('é'))
call assert_equal(9731, char2nr('☃'))
call assert_equal('A', nr2char(65))
call assert_equal('é', nr2char(233))
call assert_equal('☃', nr2char(9731))
call assert_equal('☃', nr2char(char2nr('☃')))

" --- trim(): default strips whitespace (space, tab); a mask sets the char set;
"     the {dir} arg limits stripping to 1=leading-only or 2=trailing-only
call assert_equal('hi', trim('  hi  '))
call assert_equal('hi', trim("  \t hi \t "))
call assert_equal('hi', trim('xxhixx', 'x'))
call assert_equal('hi', trim('...hi...', '.'))
call assert_equal('hi  ', trim('  hi  ', ' ', 1))
call assert_equal('  hi', trim('  hi  ', ' ', 2))

" --- escape(): prefix each listed character with a backslash
call assert_equal('a\b\/c', escape('a\b/c', '/'))
call assert_equal('c:\\path', escape('c:\path', '\'))
call assert_equal('', escape('', 'x'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'str_numconv.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'str_numconv.vim: all assertions passed'
