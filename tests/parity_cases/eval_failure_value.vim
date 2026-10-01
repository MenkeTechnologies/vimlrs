" eval() that fails still returns Number 0 to the command around it — parse
" failures and run-time evaluator failures alike — and leftover text keeps its
" leading blanks in E488.
echo eval('1 + *')
echo eval('*')
let x = eval(']')
echo x
echo eval('nosuchvar')
echo 'a' eval('[1,') 'b'
echo eval('strlen([1])')
try
  echo eval('*')
catch
  echo 'caught' v:exception
endtry
try
  let y = eval('nosuch')
catch
  echo 'caught' v:exception
endtry
echo eval('1 2')
echo eval('1 ') . '|'
echo eval("1\t2")
echo eval('{x -> x * 2}')(4)
