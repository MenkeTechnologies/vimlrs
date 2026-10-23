" The E475 of a curly-braces target whose {expr} fails is severe (get_lval
" sets emsg_severe): inside :try it is the exception thrown, replacing the
" E121 the same command raised first.
try
  let {nosuch} = 1
catch
  echo v:exception
endtry
try
  for {nosuch} in [1]
  endfor
catch
  echo v:exception
endtry
" In an expression the E121 is the exception (no E475 there).
try
  echo a{nosuch}
catch
  echo v:exception
endtry
" Two ordinary errors in one command: the first is thrown.
try
  echo nosuch1 nosuch2
catch
  echo v:exception
endtry
