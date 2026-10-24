" A branch that is not taken is read in skip mode; ex_return() outside a
" function and a block command with nothing to close still report there.
if 0
  return
else
  echo 'not run'
endif
echo 'a'
if 1
  echo 'b'
elseif 1
  return
else
  endif
endif
echo 'c'
if 0
  if 1
    endtry
  endif
endif
echo 'd'
if 0
  echo 'x'
elseif 0
  return
endif
echo 'e'
if 0 | return | endif
