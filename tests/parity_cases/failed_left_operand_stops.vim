" A binary operator whose left operand failed does not evaluate its right one:
" each eval.c level returns FAIL at once.
echo nosuch + nosuch2
echo 'a' nosuch . F() 'b'
let x = nosuch * nosuch2
echo nosuch == nosuch2
echo [nosuch + nosuch2]
echo nosuch < 1 + nosuch2
let y = 1 + nosuch + nosuch2
echo exists('x') exists('y')
echo len(nosuch . nosuch2)
echo 'end'
