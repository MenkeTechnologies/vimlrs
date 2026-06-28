" glob2regpat.vim — glob2regpat() converts a shell-style glob to a Vim regex
" (funcs.c). `*` becomes `.*`, `?` becomes `.`, `.` is escaped, and the result is
" anchored so it matches the whole string. (Vim omits a redundant `^` before a
" leading `*` / `$` after a trailing `*`; the regex still matches the same set,
" which is what these tests check via =~.) Self-test into v:errors.

" --- `*` matches any run; the match is whole-string anchored
call assert_equal(1, 'a.txt' =~ glob2regpat('*.txt'))
call assert_equal(1, 'deep/a.txt' =~ glob2regpat('*.txt'))
call assert_equal(0, 'a.txtx' =~ glob2regpat('*.txt'))

" --- a trailing `*` anchors the prefix
call assert_equal(1, 'foobar' =~ glob2regpat('foo*'))
call assert_equal(0, 'afoo' =~ glob2regpat('foo*'))

" --- `?` matches exactly one character
call assert_equal(1, 'axb' =~ glob2regpat('a?b'))
call assert_equal(0, 'ab' =~ glob2regpat('a?b'))
call assert_equal(0, 'axxb' =~ glob2regpat('a?b'))

" --- `.` is taken literally (escaped), and a [class] passes through
call assert_equal(1, 'a.c' =~ glob2regpat('a.c'))
call assert_equal(0, 'axc' =~ glob2regpat('a.c'))
call assert_equal(1, 'b.txt' =~ glob2regpat('[abc].txt'))

" --- a glob with no wildcards matches exactly
call assert_equal(1, 'plain' =~ glob2regpat('plain'))
call assert_equal(0, 'plainer' =~ glob2regpat('plain'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'glob2regpat.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'glob2regpat.vim: all assertions passed'
