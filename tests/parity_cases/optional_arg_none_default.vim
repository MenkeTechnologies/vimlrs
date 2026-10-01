" An optional argument passed as v:none takes its default (call_user_func).
function! Def(a, b = 10, c = 'c', ...)
  return [a:a, a:b, a:c, a:000]
endfunction
echo Def(1) Def(1, 2) Def(1, v:none, 3) Def(1, v:none, v:none, v:none)
echo Def(1, v:null) Def(1, 0, v:none)
function! NoDef(a)
  return a:a
endfunction
echo NoDef(v:none)
