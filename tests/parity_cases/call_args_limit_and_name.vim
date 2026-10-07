" get_func_arguments() reads at most MAX_FUNC_ARGS (20) arguments; a list not
" closed by then is E740, not E116. Only lines where vim and Neovim word the
" error the same are here: in an expression vim quotes the source from the
" function name on and Neovim prints the bare name, and this port follows
" Neovim (README: the vendored C is the spec), so those lines are not a vim
" parity case. :call names the bare function in both.
func! F(...)
  return a:0
endfunc
echo F(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20)
echo 'a'
call F(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21)
echo 'b'
echo 'c'
echo 'd'
echo [1]->F(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20)
echo 'e'
call F(1 2)
echo 'f'
echo 'g'
