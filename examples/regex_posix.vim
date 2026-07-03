" regex_posix.vim — POSIX bracket-expression classes `[[:name:]]` inside `[...]`.
" Per :help /[:alpha:], Vim supports the standard POSIX classes plus the extras
" tab/escape/backspace/return/ident/keyword. ASCII-ness matters and is NOT
" uniform: [:alpha:] [:alnum:] [:digit:] [:xdigit:] [:graph:] [:punct:] [:cntrl:]
" are ASCII-only (like \a), but [:lower:]/[:upper:] are Unicode-case-aware
" (é/ÿ/À/Ω match, unlike ASCII-only \l/\u) and [:print:] is multibyte-aware.
" Every expected value below was verified against nvim 0.12.3 and vim 9.2.
" Self-test: asserts into v:errors, throws if any failed.

" --- [:alnum:] [0-9A-Za-z] (no `_`, unlike \w; ASCII-only)
call assert_equal('a1', matchstr('a1_z', '[[:alnum:]]\+'))
call assert_equal('h', matchstr('héllo', '[[:alnum:]]\+'))

" --- [:alpha:] [A-Za-z] (ASCII-only: accented/Greek/CJK excluded)
call assert_equal('ab', matchstr('éab', '[[:alpha:]]\+'))
call assert_equal(-1, match('éÀΩ中', '[[:alpha:]]'))

" --- [:digit:] [0-9] (ASCII-only: fullwidth ４ excluded)
call assert_equal('12', matchstr('a12４5', '[[:digit:]]\+'))
" --- [:xdigit:] [0-9A-Fa-f]
call assert_equal('9aF', matchstr('gG9aF', '[[:xdigit:]]\+'))

" --- [:lower:] Unicode lowercase (é ÿ qualify, unlike ASCII-only \l)
call assert_equal('abÿ', matchstr('abÿC', '[[:lower:]]\+'))
call assert_equal(0, match('é', '[[:lower:]]'))
" --- [:upper:] Unicode uppercase (À Ω qualify, unlike ASCII-only \u)
call assert_equal('ABÀΩ', matchstr('ABÀΩmn', '[[:upper:]]\+'))
call assert_equal(0, match('À', '[[:upper:]]'))

" --- [:blank:] space or tab only
call assert_equal(" \t", matchstr("x \ty", '[[:blank:]]\+'))
" --- [:space:] full POSIX whitespace incl vertical-tab (0x0B), unlike \s
call assert_equal('  ', matchstr('x  y', '[[:space:]]\+'))
call assert_equal(0, match("\x0b", '[[:space:]]'))

" --- [:cntrl:] control chars 0x00-0x1F + DEL 0x7F (é is not control)
call assert_equal(nr2char(1), matchstr("a\x01b", '[[:cntrl:]]'))
call assert_equal(-1, match('aé', '[[:cntrl:]]'))

" --- [:graph:] ASCII printable non-space 0x21-0x7E (space and é excluded)
call assert_equal('a!', matchstr(' a! ', '[[:graph:]]\+'))
call assert_equal(-1, match('é', '[[:graph:]]'))
" --- [:print:] multibyte-aware printable (space AND é included, DEL not)
call assert_equal(' a! ', matchstr(' a! ', '[[:print:]]\+'))
call assert_equal(0, match('é', '[[:print:]]'))

" --- [:punct:] ASCII punctuation (includes `_`; é/space excluded)
call assert_equal('!@#', matchstr('a!@#b', '[[:punct:]]\+'))
call assert_equal(0, match('_', '[[:punct:]]'))

" --- Vim extras: single-char classes
call assert_equal(nr2char(9), matchstr("a\tb", '[[:tab:]]'))
call assert_equal(nr2char(27), matchstr("a\eb", '[[:escape:]]'))
call assert_equal(nr2char(8), matchstr("a\bb", '[[:backspace:]]'))
call assert_equal(nr2char(13), matchstr("a\rb", '[[:return:]]'))

" --- [:ident:] 'isident' (ASCII letters/digits/_ + bytes 0xC0-0xFF; single-byte)
"     × (U+00D7) qualifies via the 0xC0-0xFF range; CJK 中 does not.
call assert_equal('x_9', matchstr('中x_9', '[[:ident:]]\+'))
call assert_equal('×', matchstr('中×中', '[[:ident:]]'))
" --- [:keyword:] 'iskeyword' is multibyte-aware, so 中 qualifies
call assert_equal('中x', matchstr('中x 中', '[[:keyword:]]\+'))

" --- Negation: [^[:alpha:]] matches the first non-letter
call assert_equal('1', matchstr('a1', '[^[:alpha:]]'))
" --- Negated composition of two classes
call assert_equal('a', matchstr('a b1', '[^[:digit:][:space:]]\+'))

" --- Composition with a literal range: [[:digit:]a-f]
call assert_equal('f3', matchstr('gf3', '[[:digit:]a-f]\+'))
" --- Composition of two classes: [[:alpha:][:space:]]
call assert_equal('a b', matchstr('a b1', '[[:alpha:][:space:]]\+'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'regex_posix.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'regex_posix.vim: all assertions passed'
