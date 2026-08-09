" :unlet on a missing variable raises E108; only :unlet! is silent.
let g:a = 1
echo 'start'
try
  unlet g:nope
catch
  echo 'caught: ' . v:exception
endtry
unlet! g:nope
echo 'after bang'
try
  unlet g:a g:nope2 g:a3
catch
  echo 'caught2: ' . v:exception
endtry
echo 'exists a: ' . exists('g:a')
let g:b = 2
unlet g:b
echo 'exists b: ' . exists('g:b')
function! F()
  let x = 1
  unlet x
  try
    unlet x
  catch
    echo 'caught3: ' . v:exception
  endtry
  unlet! x
  echo 'fn done'
endfunction
call F()
echo 'done'
