" v:throwpoint — the location an exception was raised at. Recorded as a
" boolean test rather than the string itself, because vim's string embeds the
" absolute path of this file and the whole sourcing chain.
try
  throw 'x'
catch
  echo 'direct' (v:throwpoint != '')
endtry
function! Thrower()
  throw 'inner'
endfunction
try
  call Thrower()
catch
  echo 'in-function' (v:throwpoint != '')
endtry
try
  echo undefined_zzz
catch
  echo 'from-error' (v:throwpoint != '')
endtry
