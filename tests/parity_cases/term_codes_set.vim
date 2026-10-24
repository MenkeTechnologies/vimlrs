" :set of a t_xx name: a termcap option (t_AB) shows its value, a key code
" that is not set is E846 when asked for (c: do_set_option, find_key_option),
" a shorter t_ name is E518, and t_xx=value sets a key code.
for c in ['set t_zz', 'set t_zz?', 'set t_k1', 'set t_', 'set t_z', 'set t_#4?',
      \ 'set t_@7?', 'set t_AB?', 'set t_Co', 'set t_zz=x', 'set t_zz?', 'set t_xy=',
      \ 'set t_xy?', 'set not_zz', 'set t_aé?', 'set t_aé=1']
  try
    exe c
    echo c 'ok'
  catch
    echo c v:exception
  endtry
endfor
set t_Co t_zz?
echo 'end'
