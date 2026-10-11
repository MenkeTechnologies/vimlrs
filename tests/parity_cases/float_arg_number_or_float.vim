" The float builtins read their argument with tv_get_float_chk(): any type but
" Number/Float is the one blanket E808, not the per-type E89x table that
" arithmetic (tv_get_float) reports. pow/fmod/atan2 stop at the first bad one.
for e in ["float2nr('x')", "float2nr([])", "round('1.5')", "sqrt({})",
      \ "floor(v:true)", "ceil(v:null)", "trunc(function('len'))", "exp(0z00)",
      \ "log10('x')", "pow('2', [])", "fmod(1, 'x')", "atan2([], {})"]
  try
    let r = eval(e)
    echo e 'no error' r
  catch
    echo e v:exception
  endtry
endfor
" Outside a :try the failed call still yields its C default.
let r = float2nr('1')
echo r type(r)
let r = sqrt('4')
echo r type(r)
let r = pow(2, 'x')
echo r type(r)
