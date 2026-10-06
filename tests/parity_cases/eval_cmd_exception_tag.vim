" An error inside :eval is tagged Vim(eval) in the exception.
try
  eval [1][5]
catch
  echo v:exception
endtry
try
  echo 1
  eval nosuch
catch
  echo v:exception
endtry
eval [1, 2]->add(3)
let l = [1] | eval l->add(2) | echo l
try
  eval 1 +
catch
  echo v:exception
endtry
function! F()
  eval [][0]
endfunction
try | call F() | catch | echo v:exception v:throwpoint =~ 'F' | endtry
