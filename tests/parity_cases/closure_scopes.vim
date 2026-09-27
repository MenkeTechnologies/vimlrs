" A lambda or a `:function … closure` reads and writes the variables of the
" function activation it was created in, by reference (userfunc.c
" register_closure / find_var_in_scoped_ht). A lambda is a closure only when
" its body reads a name that is a local or an argument when it is created
" (check_vars); at script level nothing is captured.
function! MakeCounter()
  let n = 0
  function! Inc() closure
    let n += 1
    return n
  endfunction
  return funcref('Inc')
endfunction
let C = MakeCounter()
echo C() C() C()
echo Inc()
function! Outer()
  let x = 10
  function! Inner() closure
    return x
  endfunction
  return Inner()
endfunction
echo Outer()
function! Mk(n)
  return {-> a:n * 2}
endfunction
let F1 = Mk(3)
let F2 = Mk(5)
echo F1() F2()
function! Acc()
  let acc = []
  let Push = {v -> add(acc, v)}
  call Push(1)
  call Push(2)
  return acc
endfunction
echo Acc()
function! Late()
  let F = {-> v}
  let v = 42
  return F()
endfunction
echo Late()
function! Two()
  let a = 1
  let F = {-> a}
  let a = 2
  return F()
endfunction
echo Two()
function! Nest()
  let a = 1
  let F = {-> {-> a + 1}}
  return F()()
endfunction
echo Nest()
function! Shadow()
  let x = 1
  let F = {x -> x + 100}
  return F(5)
endfunction
echo Shadow()
function! Mod()
  let d = {'k': 0}
  let F = {-> extend(d, {'k': d.k + 1})}
  call F()
  call F()
  return d
endfunction
echo Mod()
function! Gen()
  let fns = []
  for i in range(3)
    call add(fns, {-> i})
  endfor
  return map(fns, {_, F -> F()})
endfunction
echo Gen()
function! EvalLambda()
  let q = 8
  return eval('{-> q}')()
endfunction
echo EvalLambda()
function! ExeLambda()
  let q = 7
  execute 'let G = {-> q}'
  return G()
endfunction
echo ExeLambda()
function! TN()
  let a = 1
  return typename({-> a})
endfunction
echo TN() typename({-> 1})
for i in range(3)
  let G = {-> i}
endfor
echo G()
let x = 5
let G2 = {-> x}
echo G2()
let s:sv = 5
echo {-> s:sv}()
