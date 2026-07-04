" substitute_edge.vim — Vim's quirky edges of substitute()/vim_regsub, verified
" against nvim 0.12.3 and vim 9.2. Two behaviours that are easy to get wrong:
"
"   1. Empty-match handling in a global substitute. Vim's do_string_sub skips an
"      empty match that lands on the same position as the previous empty match
"      (copy one char, advance) instead of emitting a duplicate replacement — so
"      `substitute('aaa','a*','X','g')` is `X`, not `XX`.
"   2. Replacement special chars in vim_regsub: `\n` inserts a NUL (0x00), `\r` a
"      carriage return (0x0d), `\t` a tab, `\\` a backslash. Note `\n` is a NUL
"      here, the OPPOSITE of the pattern side where `\n` means newline.
"
" Self-test into v:errors.

" --- empty match after a non-empty match is suppressed (a* eats 'aaa', then the
"     trailing empty match at end-of-string is skipped)
call assert_equal('X', substitute('aaa', 'a*', 'X', 'g'))
call assert_equal('X', substitute('a', 'a*', 'X', 'g'))
call assert_equal('X', substitute('', 'a*', 'X', 'g'))

" --- an empty match that is NOT at the previous empty-match position is kept, so
"     runs of 'a' collapse to one X but the gaps still get their own empty-match X
call assert_equal('XXbX', substitute('aab', 'a*', 'X', 'g'))
call assert_equal('XXbX', substitute('aabaa', 'a*', 'X', 'g'))
call assert_equal('XxXXxX', substitute('xaax', 'a*', 'X', 'g'))
call assert_equal('XbX', substitute('b', 'a*', 'X', 'g'))
call assert_equal('-a--b--c-', substitute('aXbXc', 'X*', '-', 'g'))

" --- all-empty pattern still fires between every char (no regression)
call assert_equal('XaXaXaX', substitute('aaa', '', 'X', 'g'))
call assert_equal('-a-b-c-', substitute('abc', 'x*', '-', 'g'))
call assert_equal('X', substitute('', '', 'X', 'g'))

" --- plain matches, non-global, and empty replacement (no regression)
call assert_equal('bXnXnX', substitute('banana', 'a', 'X', 'g'))
call assert_equal('abc', substitute('aXbXc', 'X', '', 'g'))
call assert_equal('f0obar', substitute('foobar', 'o', '0', ''))
call assert_equal('f00bar', substitute('foobar', 'o', '0', 'g'))
call assert_equal('Xabc', substitute('abc', '', 'X', ''))

" --- backreferences in the replacement still work
call assert_equal('2420', substitute('2024', '\(..\)\(..\)', '\2\1', ''))
call assert_equal('06/2024', substitute('2024-06', '\(\d\+\)-\(\d\+\)', '\2/\1', ''))

" --- replacement special chars (strtrans renders NUL as ^@, CR as ^M, tab as ^I)
call assert_equal('a^@b', strtrans(substitute('x', 'x', 'a\nb', '')))
call assert_equal('a^Mb', strtrans(substitute('x', 'x', 'a\rb', '')))
call assert_equal('a^Ib', strtrans(substitute('x', 'x', 'a\tb', '')))
call assert_equal('a\b', substitute('x', 'x', 'a\\b', ''))

" --- `\n` inserts a real NUL byte, so the length is 3 (a, NUL, b)
call assert_equal(3, len(substitute('x', 'x', 'a\nb', '')))

" --- `\r` inserts a carriage return (0x0d), not a newline
call assert_equal(13, char2nr(substitute('x', 'x', 'a\rb', '')[1]))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'substitute_edge.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'substitute_edge.vim: all assertions passed'
