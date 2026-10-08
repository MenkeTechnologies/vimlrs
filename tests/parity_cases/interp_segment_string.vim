" eval_one_expr_in_str converts each {expr} with typval2string(tv, false): a
" List or Dict as string(), anything else through tv_get_string, so a Blob is
" E976 and a Funcref E729 while the rest of the string is still built.
echo $'{1.5}|{-7}|{v:true}|{v:null}|{[1, 'a']}|{ {'k': 'v'} }|{'s'}'
echo $"{[0z01]}"
let x = 0z01
echo $'a{x}b'
echo 'm1'
echo $'a{function("len")}b'
echo 'm2'
let r = $'p{x}q'
echo 'r=' r
let h =<< eval END
line {x} end
END
echo h
try
  echo $'t{x}'
catch
  echo 'caught' v:exception
endtry
