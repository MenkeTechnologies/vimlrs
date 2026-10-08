" throw_exception() refuses a user value that would pass for an error or
" interrupt exception: "Vim" followed by NUL, ':' or '(' is E608.
for v in ['Vim', 'Vim:x', 'Vim(echo):E1: y', 'Vimx', 'vim:x', ' Vim:x', 'Vi']
  try
    throw v
  catch
    echo string(v) '->' v:exception
  endtry
endfor
try
  throw 'Vim:Interrupt'
catch /^Vim:Interrupt$/
  echo 'faked interrupt'
catch
  echo 'refused' v:exception
endtry
function! T()
  throw 'Vim:in function'
  echo 'after throw'
endfunction
try | call T() | catch | echo v:exception | endtry
throw 'Vim:uncaught'
echo 'end'
