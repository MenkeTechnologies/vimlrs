" An exception thrown by the command line execute() runs, while the caller is
" inside a :try, propagates with its own value: do_cmdline() only reports an
" uncaught exception when trylevel is 0 (ex_docmd.c). It was wrapped as
" "E605: Exception not caught: ..." here.
try
  call execute('throw "plain"')
catch
  echo 'A' v:exception
endtry
try
  call execute('try | throw "inner" | endtry')
catch
  echo 'B' v:exception
endtry
try
  call execute('try | throw "open-try"')
catch
  echo 'C' v:exception
endtry
try
  call execute(['try', 'throw "list"', 'endtry'])
catch /^list$/
  echo 'D' v:exception
endtry
try
  execute 'throw "excmd"'
catch
  echo 'E' v:exception
endtry
let l = [1]
lockvar l
try
  call execute('try | call add(l, 3)')
catch
  echo 'F' v:exception
endtry
unlockvar l
" The value a caught exception had inside execute() is not reported again.
echo execute('try | throw "x" | catch | echo "caught" v:exception | endtry')
echo 'end'
