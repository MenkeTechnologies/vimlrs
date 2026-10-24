" ex_call() finds the next command only when the call did not fail
" (userfunc.c:7000): the rest of the line, block commands included, is
" dropped, and the script runs in skip mode until no conditional is open.
if 1 | call Nosuch() | endif
echo 'skipped 1'
endif
echo 'a'
if 1
  if 1 | call Nosuch() | endif
  echo 'skipped 2'
endif
echo 'skipped 3'
endif
echo 'b'
if 1 | call Nosuch() | endif
while 1
  echo 'skipped 4'
  break
endwhile
function F()
  endif
endfunction
return
endif
echo 'c'
call Nosuch() | if 1 | echo 'skipped 5' | endif
echo 'd'
if 1 | call len([]) | endif
echo 'e'
try
  if 1 | call Nosuch() | endif
  echo 'skipped 6'
catch
  echo 'caught'
endtry
echo 'f'
if 1 | call Nosuch() | endif | echo 'skipped 7'
echo 'skipped 8'
