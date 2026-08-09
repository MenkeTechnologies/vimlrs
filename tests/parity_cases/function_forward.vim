" :function is an ordinary command: the name does not resolve until its line has
" run, so a forward reference is E700 and not a hoisted definition.
try
  let F = function('Later')
  echo 'accepted'
catch
  echo 'caught: ' . v:exception
endtry
echo 'exists before: ' . exists('*Later')
function! Later()
  return 7
endfunction
echo 'exists after: ' . exists('*Later')
echo 'now: ' . string(function('Later'))
echo 'call: ' . Later()
" A function defined later is still callable once its line has run.
function! Two()
  return One() + 1
endfunction
function! One()
  return 1
endfunction
echo 'mutual: ' . Two()
