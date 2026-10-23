" Curly-braces names: `{expr}` inside a variable or function name is evaluated
" when the command runs and spliced into the name (make_expanded_name).
let n = 'foo'
let {n} = 5
echo foo
let a{n}b = 1
echo afoob
echo {n}
echo a{n}b
let g:x_{n} = 'gx'
echo g:x_foo
let i = 3
let var{i} = i * 2
echo var3
" Compound assignment and unlet through an expanded name.
let w = 'x'
let a{w} = 1
let a{w} += 5
echo ax
unlet a{w}
echo exists('ax')
" A function defined and called under an expanded name, global and s:.
let s:nm = 'Hi'
function! F{n}()
  return 'in Ffoo'
endfunction
echo Ffoo()
echo F{n}()
call F{n}()
function! s:{s:nm}()
  return 'in s:Hi'
endfunction
echo s:Hi()
echo s:{s:nm}()
" The expanded name may be a builtin.
let b = 'len'
echo {b}('abc')
" Expansion recurses while the result still holds braces.
let m = 'x'
let x = 9
let r = '{m}'
echo {r}
echo {'x'}
" Dict-literal shapes are still dicts; a name is one expression then `}`.
let d = {'k': 1}
echo d
let lst = ['a', 'b']
echo lst[{n} == 5 ? 0 : 1]
let [p{n}, q] = [1, 2]
echo pfoo q
" Inside a function the expanded name resolves in the function's scope.
function! Scope()
  let loc = 'q'
  let {loc} = 'local q'
  echo q
  echo l:q
  let g:{loc} = 'global q'
  return a:{'0'}
endfunction
echo Scope()
echo g:q
" A failing `{expr}`: the read fails with the inner error only; `:let` and
" `:unlet` add E475 quoting the argument; a non-name expansion is E461/E121.
echo {nosuch}
let {nosuch2} = 1
unlet {nosuch3}
echo F{nosuch4}()
echo string({nosuch5})
echo {1}
let bad = 'a b'
let {bad} = 1
let l2 = [1]
let {l2} = 1
echo {l2}
