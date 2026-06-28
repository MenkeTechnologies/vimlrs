" strings.vim — string builtins and the Vim-magic regex engine, with tests.
"
" Demonstrates: split()/join(), case folding, printf(), substitute(), the =~
" match operator and matchstr(), plus assert_match() for pattern assertions.
" Exits non-zero on failure.
"
"   vimlrs examples/strings.vim

let s = 'The Quick Brown Fox'
let line = 'error: file not found (code 404)'

" ── unit tests ──
call assert_equal('THE QUICK BROWN FOX', toupper(s))
call assert_equal('the quick brown fox', tolower(s))
call assert_equal(['The', 'Quick', 'Brown', 'Fox'], split(s))
call assert_equal('The-Quick-Brown-Fox', join(split(s), '-'))
call assert_equal(19, strlen(s))
call assert_equal('Fox Brown Quick The', join(reverse(split(s)), ' '))
call assert_equal('hex ff  pad 00042  float 3.142', printf('hex %x  pad %05d  float %.3f', 255, 42, 3.14159))
call assert_true(line =~ '\d\+')
call assert_equal('404', matchstr(line, '\d\+'))
" matchlist() returns the whole match plus the nine \1..\9 submatch slots.
call assert_equal(['a-b', 'a', 'b', '', '', '', '', '', '', ''], matchlist('a-b', '\(\w\)-\(\w\)'))
call assert_equal('error: file not found (code ###)', substitute(line, '\d\+', '###', 'g'))
call assert_match('^error:', line)
call assert_notmatch('^\d', line)

" ── printf conversions: octal/binary/char/scientific ──
call assert_equal('ff 10 101', printf('%x %o %b', 255, 8, 5))
call assert_equal('Hi!', printf('%c%c%c', 72, 105, 33))
call assert_equal('00000101', printf('%08b', 5))
call assert_equal('1.234568e+04', printf('%e', 12345.678))

" ── printf sign flags: + forces a sign, space pads positives ──
call assert_equal('+7', printf('%+d', 7))
call assert_equal(' 7', printf('% d', 7))
call assert_equal('+0007', printf('%+05d', 7))
call assert_equal('+3.10', printf('%+.2f', 3.1))

" ── len() of a String is its BYTE length (multibyte counts each byte) ──
call assert_equal(6, len('héllo'))
call assert_equal(5, strchars('héllo'))

" ── trim() honours the {dir}: 0/none = both ends, 1 = left, 2 = right ──
call assert_equal('x', trim('  x  '))
call assert_equal('x  ', trim('  x  ', ' ', 1))
call assert_equal('  x', trim('  x  ', ' ', 2))
call assert_equal('hixx', trim('xxhixx', 'x', 1))
call assert_equal('abc', trim('abc', ''))

" ── printf %g: 6 significant digits, %e/%f chosen by exponent ──
call assert_equal('0.1 1e+06 0.0001', printf('%g %g %g', 0.1, 1000000.0, 0.0001))
call assert_equal('3.14', printf('%.3g', 3.14159))
call assert_equal('1.5E-10', printf('%G', 1.5e-10))

" ── substitute() with a \= replacement EXPRESSION + submatch() ──
call assert_equal('ABCABC', substitute('abcABC', '[a-z]', '\=toupper(submatch(0))', 'g'))
call assert_equal('x11y12', substitute('x1y2', '\d', '\=submatch(0)+10', 'g'))
call assert_equal('ba-c', substitute('a-b-c', '\(\w\)-\(\w\)', '\=submatch(2).submatch(1)', 'g'))
call assert_equal('price: 200', substitute('price: 100', '\d\+', '\=submatch(0)*2', ''))

" ── substitute() case escapes: \u \l \U \L ──
call assert_equal('Hello World', substitute('hello world', '\w\+', '\u\0', 'g'))
call assert_equal('hello', substitute('HELLO', '.*', '\L\0', ''))
call assert_equal('HELLO', substitute('hello', '.*', '\U\0', ''))
call assert_equal('fooBar', substitute('FooBar', '.', '\l\0', ''))

" ── split() keeps internal empty items, drops leading/trailing ──
call assert_equal(['a', 'b', '', 'c'], split('a,b,,c', ','))
call assert_equal(['a', 'b'], split(',a,b,', ','))
call assert_equal(['', 'a', 'b', ''], split(',a,b,', ',', 1))

" ── str2nr() honours an explicit base (2/8/16), with/without prefix ──
call assert_equal(255, str2nr('ff', 16))
call assert_equal(255, str2nr('0xff', 16))
call assert_equal(5, str2nr('101', 2))
call assert_equal(15, str2nr('17', 8))
call assert_equal(-42, str2nr('  -42  '))
call assert_equal(0, str2nr('0xff'))

" ── demo ──
echo 'upper    :' toupper(s)
echo 'words    :' split(s)
echo 'matchstr :' matchstr(line, '\d\+')
echo 'censored :' substitute(line, '\d\+', '###', 'g')

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: strings assertions passed'
