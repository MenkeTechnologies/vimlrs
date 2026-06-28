" fib.vim — Fibonacci two ways (recursion + hot loop), with embedded unit tests.
"
" Demonstrates: :function/:return, recursion with a:/l: scopes, an accumulator
" while-loop that trace-JIT-compiles, and the built-in assert framework.
" Exits non-zero if any assertion fails. Run with VIMLRS_JIT_STATS=1 to see the
" loop trace get compiled.
"
"   vimlrs examples/fib.vim

function! Fib(n) abort
  if a:n < 2
    return a:n
  endif
  return Fib(a:n - 1) + Fib(a:n - 2)
endfunction

" Iterative — a tight numeric loop (the JIT's home turf).
function! FibIter(n) abort
  let a = 0
  let b = 1
  let i = 0
  while i < a:n
    let t = a + b
    let a = b
    let b = t
    let i += 1
  endwhile
  return a
endfunction

" ── unit tests: both implementations agree, on known values ──
let expected = [0, 1, 1, 2, 3, 5, 8, 13, 21, 34]
for i in range(len(expected))
  call assert_equal(expected[i], Fib(i))
  call assert_equal(expected[i], FibIter(i))
endfor
call assert_equal(55, FibIter(10))
call assert_equal(12586269025, FibIter(50))

" ── demo ──
echo 'fib(0..9):' map(range(10), 'Fib(v:val)')
echo printf('fib(50) = %d', FibIter(50))

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: fib assertions passed'
