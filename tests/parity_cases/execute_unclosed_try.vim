" :execute runs its text as a command line (ex_execute -> do_cmdline): an
" unclosed :try ends with the text, so its exception reaches the caller.
function F()
  exe 'try | throw "z"'
endfunction
try
  call F()
catch
  echo 'caught' v:exception
endtry
try
  exe 'try | throw "w" | endtry'
catch
  echo 'caught' v:exception
endtry
exe 'try | echo "body" | catch | echo "no" | endtry'
echo 'end'
