" fib.vim — Fibonacci two ways: recursion and a hot numeric loop.
"
" Demonstrates: :function/:return, recursion with a:/l: scopes, and an
" accumulator while-loop whose provably-Number locals trace-JIT-compile to
" native code. Run with VIMLRS_JIT_STATS=1 to see the loop trace get compiled.
"
"   vimlrs examples/fib.vim
"   VIMLRS_JIT_STATS=1 vimlrs examples/fib.vim

function! Fib(n) abort
  if a:n < 2
    return a:n
  endif
  return Fib(a:n - 1) + Fib(a:n - 2)
endfunction

echo 'recursive:'
for i in range(10)
  echo printf('  fib(%d) = %d', i, Fib(i))
endfor

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

echo 'iterative:'
echo printf('  fib(50) = %d', FibIter(50))
