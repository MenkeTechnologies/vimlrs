" regex_classes.vim — Vim regex character-class atoms are ASCII-only.
" Per :help /\a, the atoms \a \A \l \u \w \W \d \x are defined over ASCII
" ranges ([A-Za-z], [a-z], [A-Z], [0-9A-Za-z_], [0-9], [0-9A-Fa-f]) and do NOT
" match multibyte letters/digits (é À Ω ４), no matter the locale. Only the
" word-boundary atoms \< \> follow 'iskeyword' and DO treat multibyte letters as
" keyword chars. Every expected value below was verified against nvim 0.12 and
" vim 9.2. Self-test: asserts into v:errors, throws if any failed.

" --- \a [A-Za-z]: stops at the first non-ASCII letter
call assert_equal('h', matchstr('héllo', '\a\+'))
call assert_equal('h', matchstr('héllo À Ω 123 ４２', '\a\+'))
" --- \A (non-\a): a multibyte letter IS a non-\a char
call assert_equal('é', matchstr('héllo', '\A\+'))

" --- \w [0-9A-Za-z_]: multibyte letters/digits are NOT word chars
call assert_equal('ab', matchstr('abÿ123', '\w\+'))
call assert_equal('h', matchstr('h４i', '\w\+'))
" --- \W (non-\w): the multibyte run
call assert_equal('Àé', matchstr('aÀé', '\W\+'))

" --- \l [a-z]: accented lowercase (é ÿ) does not qualify
call assert_equal('x', matchstr('xÀÿz', '\l\+'))
call assert_equal('ab', matchstr('abÿc', '\l\+'))
" --- \u [A-Z]: accented uppercase (À) and Greek (Ω) do not qualify
call assert_equal('AB', matchstr('ABÀmn', '\u\+'))
call assert_equal('Ω', matchstr('!Ω', '\<.'))

" --- \d [0-9]: fullwidth digit ４ is not an ASCII digit
call assert_equal('12', matchstr('12４5', '\d\+'))
" --- \x [0-9A-Fa-f]: é breaks the hex run
call assert_equal('caf', matchstr('caféF00', '\x\+'))

" --- substitute() over the class atoms (per-char): only ASCII letters replaced
call assert_equal('XéXXX', substitute('héllo', '\a', 'X', 'g'))
call assert_equal('_À_', substitute('aÀb', '\w', '_', 'g'))

" --- match() index: first non-\a char is é at index 1
call assert_equal(1, match('héllo', '\A'))

" --- word boundary \< IS multibyte-aware (unlike \a): Ω is a keyword char, so
"     the \< anchor fires before the ASCII word 'word' after a space
call assert_equal('word', matchstr('!Ω word', '\<\a\+'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'regex_classes.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'regex_classes.vim: all assertions passed'
