" A function-local read BY NAME — a map()/filter() string, eval() reached
" through a call — sees the local, which therefore cannot live outside the
" l: dict.
function! StrMap()
  let k = 4
  return map([1, 2], 'v:val * k')
endfunction
echo StrMap()
function! StrFilter()
  let lim = 2
  return filter([1, 2, 3], 'v:val >= lim')
endfunction
echo StrFilter()
function! MethodMap()
  let k = 3
  return [1, 2]->map('v:val + k')
endfunction
echo MethodMap()
function! EvalCall()
  let q = 8
  let R = eval('{-> 1}')() + eval('q')
  return R
endfunction
echo EvalCall()
function! Subst()
  let n = 5
  return substitute('a', 'a', '\=n', '')
endfunction
echo Subst()
