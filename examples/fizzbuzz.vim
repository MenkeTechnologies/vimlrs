" fizzbuzz.vim — classic FizzBuzz in standalone VimL.
"
" Demonstrates: :for over range(), :if/:elseif/:else, integer % (native modulo),
" string concatenation, and :echo. The numeric `for i in range(...)` loop
" trace-JIT-compiles to a native counter loop.
"
"   vimlrs examples/fizzbuzz.vim

for i in range(1, 20)
  if i % 15 == 0
    echo 'FizzBuzz'
  elseif i % 3 == 0
    echo 'Fizz'
  elseif i % 5 == 0
    echo 'Buzz'
  else
    echo i
  endif
endfor
