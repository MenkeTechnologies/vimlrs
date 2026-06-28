" glob.vim — list files matching a wildcard pattern, with embedded unit tests.
"
" Demonstrates glob(): expand a file pattern to its matches, as a newline-joined
" String or (with the {list} arg) a List; `*` and `?` wildcards; `$VAR`/`~`
" expansion; and literal paths. Sets up a known temp directory so the assertions
" are deterministic, then cleans up. Self-checks and exits non-zero on failure.
"
"   vimlrs examples/glob.vim

" Build a throwaway directory with known contents.
let dir = tempname()
call mkdir(dir)
call writefile(['a'], dir . '/alpha.txt')
call writefile(['b'], dir . '/beta.txt')
call writefile(['c'], dir . '/gamma.log')

" ── unit tests ──
" List form: matches are sorted.
call assert_equal([dir . '/alpha.txt', dir . '/beta.txt'], glob(dir . '/*.txt', 0, 1))
call assert_equal(3, len(glob(dir . '/*', 0, 1)))
call assert_equal(1, len(glob(dir . '/*.log', 0, 1)))
" '?' matches exactly one character.
call assert_equal([dir . '/gamma.log'], glob(dir . '/gamm?.log', 0, 1))
" No match -> empty List / empty String.
call assert_equal([], glob(dir . '/none.*', 0, 1))
call assert_equal('', glob(dir . '/none.*'))
" String form is newline-joined.
call assert_equal(2, len(split(glob(dir . '/*.txt'), "\n")))
" A literal (non-wildcard) path yields itself iff it exists.
call assert_equal(dir . '/alpha.txt', glob(dir . '/alpha.txt'))
call assert_equal('', glob(dir . '/missing.txt'))
" $VAR is expanded before matching.
call assert_equal($HOME, glob('$HOME'))

" ── demo ──
echo 'txt files:' map(glob(dir . '/*.txt', 0, 1), 'fnamemodify(v:val, ":t")')

" cleanup
for f in glob(dir . '/*', 0, 1)
  call delete(f)
endfor
call delete(dir, 'd')

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: glob assertions passed'
