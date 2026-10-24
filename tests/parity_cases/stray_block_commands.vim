" A block command with no block to act on only reports (ex_endif(), ex_else(),
" ex_endwhile(), ex_catch(), ex_endtry()), with the command text appended.
endif
else
elseif 1
endwhile
endfor
catch /x/
finally
endtry
endfunction
en
  endif  
endif "comment
endif | echo 'not run'
if 1 |  endif  |  endif  
echo 'a'
if 1
  else
  else
  echo 'not run'
endif
if 0
else
  elseif 1
  echo 'not run'
endif
while 1
  endif
  echo 'not run'
  break
endwhile
for i in [1]
  endtry
endfor
if 1
  endwhile
endif
try
  endif
catch
  echo v:exception
endtry
function F()
  endif
  echo 'in F'
endfunction
call F()
echo 'end'
