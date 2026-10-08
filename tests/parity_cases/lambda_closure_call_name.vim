" eval_func runs check_vars on a call's NAME while a lambda body is skipped, so
" calling a Funcref local of the enclosing function makes the lambda a closure.
let Compose = {f, g -> {x -> f(g(x))}}
echo Compose({x -> x * 2}, {x -> x + 1})(3)
function! Wrap(F)
  let Inner = a:F
  return {x -> Inner(x) . '!'}
endfunction
echo Wrap(function('toupper'))('ab')
function! Meth(F)
  let Tw = a:F
  return {x -> x->Tw()}
endfunction
echo Meth({s -> s . s})('ab')
function! Late()
  let L = {-> exists('*Later') . exists('Later')}
  let Later = {-> 'late bound'}
  return L()
endfunction
echo Late()
