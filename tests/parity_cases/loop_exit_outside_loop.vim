" :break/:continue with no loop and :return outside a function are run-time
" errors; the script carries on after them.
echo 0
return 1
echo 1 | retu | echo 2
try | break | catch | echo v:exception | endtry
if 1 | contin | endif
silent! break
   continue  
echo 1 |continue|echo 2
echo 1 |  silent continue  |echo 2
echo 1 |  cont "x
try
  return
catch
  echo v:exception
endtry
for i in [1]
  break
endfor
echo execute('break')
exe 'return 5'
echo 'after'
