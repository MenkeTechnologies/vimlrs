" A subscript whose base or index FAILed is not taken (c: eval_index is not
" reached): the error is the operand's alone, with no E716 for the value the
" failed read recovered.
echo nosuch[0]
echo nosuch.x
echo [1][nosuch]
echo {}[nosuch]
echo 'abc'[nosuch]
echo [1][nosuch : 2]
echo {}[[]]
let d = {}
let d[nosuch] .= 1
echo d
echo d[nosuch]['x']
echo len(d[nosuch])
echo nosuch.a.b
let e = {'a': {'b': 1}}
echo e.a.b
let s = 'x'
let t = 'y'
echo s.t
function F()
  let l = {'k': 2}
  let i = 0
  return l.k + i
endfunction
echo F()
