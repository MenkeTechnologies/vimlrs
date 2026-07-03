" path_funcs.vim — path-string builtins (funcs.c: simplify/resolve/
" isabsolutepath, and the two shell/filename escapers escape/fnameescape).
" These are pure string transforms — no filesystem access needed for the cases
" asserted here (resolve() of a non-symlink relative path returns it unchanged,
" so the result is stable across platforms). Self-test: throws if any failed.

" --- simplify(): collapse '.', '..' and duplicate slashes textually
call assert_equal('bar', simplify('foo/../bar'))
call assert_equal('foo/baz', simplify('foo/./bar/../baz'))
call assert_equal('/c', simplify('/a/b/../../c'))
call assert_equal('a/b', simplify('a//b'))
call assert_equal('a/b/c', simplify('a/b/c'))
call assert_equal('/x', simplify('/../x'))

" --- isabsolutepath(): leading '/' is absolute; relative and '.'-prefixed are not
call assert_equal(1, isabsolutepath('/usr/bin'))
call assert_equal(0, isabsolutepath('foo/bar'))
call assert_equal(0, isabsolutepath('./x'))
call assert_equal(0, isabsolutepath(''))

" --- resolve(): a relative path with no symlink components is returned as-is
call assert_equal('foo/bar', resolve('foo/bar'))

" --- fnameescape(): escape characters special to Ex commands / :edit
call assert_equal('foo\ bar.txt', fnameescape('foo bar.txt'))
call assert_equal('a\|b', fnameescape('a|b'))
call assert_equal('with\#hash\%pct', fnameescape('with#hash%pct'))
call assert_equal('plain.txt', fnameescape('plain.txt'))

" --- escape(): backslash-prefix each listed character; a literal backslash needs
" doubling in the source ('\\' is one backslash)
call assert_equal('a\\b', escape('a\b', '\'))
call assert_equal('c:\\path', escape('c:\path', '\'))
call assert_equal('a\,b\;c', escape('a,b;c', ',;'))
call assert_equal('\*.txt', escape('*.txt', '*'))
call assert_equal('', escape('', 'abc'))
call assert_equal('no-special', escape('no-special', 'xyz'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'path_funcs.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'path_funcs.vim: all assertions passed'
