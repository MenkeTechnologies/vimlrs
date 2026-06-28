" varargs.vim — variadic functions (a:000 / a:0) and :unlet, with unit tests.
"
" Demonstrates `...` varargs (all extra args collect into a:000, with a:0 their
" count) — both alone and after fixed parameters — and deleting variables with
" :unlet. Self-checks and exits non-zero on failure.
"
"   vimlrs examples/varargs.vim

" ── a function that takes only varargs ──
function! Sum(...) abort
  let total = 0
  for x in a:000
    let total += x
  endfor
  return total
endfunction

call assert_equal(0, Sum())
call assert_equal(10, Sum(1, 2, 3, 4))
call assert_equal(3, Sum(10, 20, 30) / 10 * 1)

" ── fixed params then varargs: a:000 holds only the extras ──
function! Tag(label, ...) abort
  return a:label . ': ' . a:0 . ' items'
endfunction

call assert_equal('x: 0 items', Tag('x'))
call assert_equal('y: 3 items', Tag('y', 1, 2, 3))

" ── a vararg function works as a lambda body's callee, too ──
call assert_equal([1, 3, 6], map([1, 2, 3], {i, v -> Sum(v, i * v)}))

" ── :unlet removes variables (single, multiple, and :unlet!) ──
let g:tmp = 99
call assert_true(exists('g:tmp'))
unlet g:tmp
call assert_false(exists('g:tmp'))

let g:a = 1 | let g:b = 2
unlet g:a g:b
call assert_false(exists('g:a'))
call assert_false(exists('g:b'))

" :unlet! does not error on a missing name.
let g:keep = 5
unlet! g:missing g:keep
call assert_false(exists('g:keep'))

" ── demo ──
echo 'Sum(1..5) =' Sum(1, 2, 3, 4, 5)
echo 'Tag(file, a, b) =' Tag('file', 'a', 'b')

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: varargs assertions passed'
