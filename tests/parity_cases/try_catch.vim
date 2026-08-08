try
  throw 'boom'
catch /^bo/
  echo 'caught' v:exception
endtry
try
  throw 'x'
catch /nomatch/
  echo 'wrong'
catch
  echo 'fallback' v:exception
finally
  echo 'fin'
endtry
function! Thrower()
  throw 'inner'
endfunction
try
  call Thrower()
catch /inner/
  echo 'nested' v:exception
endtry
try
  echo undefined_thing
catch /E121/
  echo 'E121 caught'
endtry
try
  try
    throw 'a'
  finally
    echo 'inner fin'
  endtry
catch
  echo 'outer' v:exception
endtry
let s = 0
for i in range(3)
  try
    if i == 1
      throw 'skip'
    endif
    let s += i
  catch
    let s += 100
  endtry
endfor
echo s
echo v:exception
