" A HARD failure abandons the command line it was parsed on, and the line
" execute() runs is a NESTED one. So the flag must not survive the nested run:
" the caller's :catch on the SAME bar-separated line still has to see the error.
"
" `{}+1` inside :echo is a hard failure, so the direct form below abandons the
" line in vim too -- that half is parity already and is here so a fix for the
" execute() half cannot quietly turn it into a catch.
try | let s:r = execute('echo {}+1') | catch | echo 'caught-let:' . v:exception | endtry
try | echo execute('echo {}+1') | catch | echo 'caught-echo:' . v:exception | endtry
" Errors that were never hard are caught on one line, with and without execute().
try | let s:r = strlen({}) | catch | echo 'caught-strlen:' . v:exception | endtry
try | let s:r = undefined_fn_xyz() | catch | echo 'caught-fn:' . v:exception | endtry
" `:silent!` around a nested hard failure still runs the rest of the line.
silent! let g:a = execute('echo {}+1') | echo 'ran-after-silent'
" The multi-line form, which was already right.
try
  let s:r = execute('echo [] + 1')
catch
  echo 'caught-multi:' . v:exception
endtry
echo 'end'
