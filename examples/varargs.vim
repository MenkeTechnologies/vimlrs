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

" ── each extra arg is also reachable positionally as a:1, a:2, ... a:N ──
" (Vim binds the varargs individually in addition to the a:000 list; e.g.
" runtime indent/html.vim's s:AddBlockTag(tag, id, ...) reads a:1.)
function! Positional(tag, id, ...) abort
  if a:0 == 0
    return a:tag . '/' . a:id
  else
    return a:tag . '/' . a:id . '/' . a:1
  endif
endfunction

call assert_equal('pre/2', Positional('pre', 2))
call assert_equal('cmt/5/-->', Positional('cmt', 5, '-->'))

function! Pick(...) abort
  return a:1 . a:2 . a:3
endfunction

call assert_equal('abc', Pick('a', 'b', 'c'))
" a:N and a:000 stay in sync.
function! Both(...) abort
  return [a:1, a:2, a:000]
endfunction
call assert_equal(['x', 'y', ['x', 'y']], Both('x', 'y'))

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
