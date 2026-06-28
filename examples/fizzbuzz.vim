" fizzbuzz.vim — classic FizzBuzz, with embedded unit tests.
"
" Demonstrates: :function/:return, :if/:elseif/:else, integer % (native modulo),
" and the built-in assert framework (assert_equal/v:errors). Run it directly;
" it exits non-zero if any assertion fails, so CI catches regressions.
"
"   vimlrs examples/fizzbuzz.vim

function! FizzBuzz(n) abort
  if a:n % 15 == 0
    return 'FizzBuzz'
  elseif a:n % 3 == 0
    return 'Fizz'
  elseif a:n % 5 == 0
    return 'Buzz'
  else
    return string(a:n)
  endif
endfunction

" ── unit tests ──
call assert_equal('1', FizzBuzz(1))
call assert_equal('Fizz', FizzBuzz(3))
call assert_equal('Buzz', FizzBuzz(5))
call assert_equal('Fizz', FizzBuzz(9))
call assert_equal('FizzBuzz', FizzBuzz(15))
call assert_equal('FizzBuzz', FizzBuzz(30))
call assert_equal('Buzz', FizzBuzz(20))

" ── demo ──
for i in range(1, 20)
  echo FizzBuzz(i)
endfor

" ── self-test epilogue: non-zero exit on any failure (CI regression gate) ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: fizzbuzz assertions passed'
