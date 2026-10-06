" get_func_arguments() reads at most MAX_FUNC_ARGS (20) arguments; a list not
" closed by then is E740, not E116. emsg_funcname() prints the name up to its
" NUL: in an expression that is the source from the name on, for :call's
" outermost function (trans_function_name's copy) the bare name.
func! F(...)
  return a:0
endfunc
echo F(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20)
echo F(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21)
echo 'a'
call F(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21)
echo 'b'
let x = F(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21) + 1
echo 'c'
echo len(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21)
echo 'd'
echo [1]->F(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20)
echo F(1 2) 'tail'
echo 'e'
call F(1 2)
echo 'f'
echo [1]->F(1 2) 'tail'
echo [1]->len(1 2) 'tail'
echo 'g'
