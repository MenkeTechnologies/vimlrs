" :echoerr reports its arguments as ONE error message: exit status 1, a
" catchable Vim(echoerr) exception, v:errmsg, :silent! — and it does not abort
" an `abort` function or skip the rest of an :if.
echo 'a'
echoerr 'boo' 1 [1, 'x'] {'k': "v"}
echo 'b'
try
  echoerr 'in try'
catch
  echo 'caught:' v:exception
endtry
echo v:errmsg
echon 'X'
echoerr 'after echon'
silent! echoerr 'quiet'
echo v:errmsg
function H() abort
  echoerr 'in H'
  echo 'H continues'
  return 5
endfunction
echo H()
if 1
  echoerr 'in if'
  echo 'if continues'
endif
echoerr 'x' nosuch 'y'
echoerr '' 'lead'
echoe function('tr') 1.5 v:true
echoerr
let r = execute(['echo 1', 'echoerr "boo"', 'echo 2'])
echo string(r)
" A failed argument ends :execute before anything runs.
execute 'echo' nosuch
echo 'done'
