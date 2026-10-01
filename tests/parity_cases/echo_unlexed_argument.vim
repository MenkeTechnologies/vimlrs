" An :echo argument that does not lex (an unterminated string) is reached in
" its turn: the arguments ahead of it print and run first. (The E114/E115
" wording itself is an engine split, so it is read through v:exception[:12].)
try
  echo 1 printf('%E')'
catch
  echo v:exception[:13]
endtry
try
  echo 2 'a' 'b
catch
  echo v:exception[:13]
endtry
try
  echo [3, 'x
catch
  echo v:exception[:13]
endtry
try
  echo 4 "x\" y
catch
  echo v:exception[:13]
endtry
try
  echo 5 | echo 'ab
catch
  echo v:exception[:13]
endtry
echo 'end'
