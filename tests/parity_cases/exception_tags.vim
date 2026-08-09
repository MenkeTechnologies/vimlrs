" The `Vim(cmd):` tag on an error-turned-exception names the ex-command that
" raised it. Only the leaf commands used to be named, so an error in a block
" opener's condition inherited whatever the previous statement had set.
"
" The same per-statement marker is where the source line for v:throwpoint is
" recorded, which is why every statement kind has to be named, not just the ones
" whose tag anyone reads.
try
  if [][0]
  endif
catch
  echo 'if     ' v:exception
endtry

try
  while [][0]
  endwhile
catch
  echo 'while  ' v:exception
endtry

try
  for x in [][0]
  endfor
catch
  echo 'for    ' v:exception
endtry

try
  echo [][0]
catch
  echo 'echo   ' v:exception
endtry

try
  let g:v = [][0]
catch
  echo 'let    ' v:exception
endtry

" `:silent` is a modifier: the tag is the command it modifies.
try
  silent echo [][0]
catch
  echo 'silent ' v:exception
endtry

" `:throw` evaluates its argument first and throws only if that succeeded — an
" error while evaluating it IS the outcome, not the value.
try
  throw [][0]
catch
  echo 'throw  ' v:exception
endtry

" Each bar-separated command on one line tags its own errors.
try | echo 'ok' | let g:w = [][0] | endtry
try
  echo 'ok' | let g:w = [][0]
catch
  echo 'bar    ' v:exception
endtry
