" A pattern that fails to compile reports its E-number, but `=~` / `!~` still
" yield a value (0 / 1) and the command around them still runs; inside a :try
" the error then becomes the exception of that command. A conditional whose
" test raised skips BOTH branches.
echo 'abc' =~ '\('
echo 'abc' !~ '\('
let g:r = 'abc' =~ '\('
echo g:r
try
  echo 'abc' =~ '\('
catch
  echo 'caught' v:exception
endtry
try
  let g:s = 'abc' !~ '\%['
catch
  echo 'caught' v:exception
endtry
echo g:s
try
  if 'abc' =~ '\('
    echo 'then'
  else
    echo 'else'
  endif
  echo 'not reached'
catch
  echo 'caught' v:exception
endtry
try
  if strlen([])
    echo 'then'
  elseif 1
    echo 'elseif'
  else
    echo 'else'
  endif
catch
  echo 'caught' v:exception
endtry
try
  while 'abc' =~ '\('
    echo 'body'
  endwhile
  echo 'not reached'
catch
  echo 'caught' v:exception
endtry
" a List operand is still a failed evaluation, not a value
try
  echo [] =~ 'a'
catch
  echo 'caught' v:exception
endtry
