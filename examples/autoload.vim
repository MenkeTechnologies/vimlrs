" autoload.vim — Vim's autoload mechanism: calling pkg#func() sources
" autoload/pkg.vim on demand, then runs the call.
"
" The example is self-contained: it writes a small autoload file under
" autoload/ (relative to the current directory, where vimlrs resolves them),
" calls into it to trigger the load, then cleans up. Self-checks.
"
"   vimlrs examples/autoload.vim   (run from the project root)

" ── set up a throwaway autoload package ──
call mkdir('autoload', 'p')
call writefile([
      \ 'function! demo#double(n) abort',
      \ '  return a:n * 2',
      \ 'endfunction',
      \ 'let g:demo_autoloaded = 1',
      \ ], 'autoload/demo.vim')

" ── the first call to demo#double sources autoload/demo.vim, then runs ──
call assert_equal(10, demo#double(5))
call assert_equal(1, g:demo_autoloaded)
" Subsequent calls reuse the already-loaded function.
call assert_equal([2, 4, 6], map([1, 2, 3], {i, v -> demo#double(v)}))

" ── demo ──
echo 'demo#double(21) ->' demo#double(21)

" ── cleanup ──
call delete('autoload/demo.vim')
call delete('autoload', 'd')

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: autoload assertions passed'
