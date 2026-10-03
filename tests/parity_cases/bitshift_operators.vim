" vim 9.2 legacy `<<` / `>>` (eval.c eval5, eval_shift_number): the level
" between the comparisons and `+`/`-`/`.`, unsigned, more than 63 bits is 0.
echo 1 << 3
echo 1 << 3 - 1
echo 256 >> 2 + 1
echo 1 << 63
echo 1 << 64
echo -8 >> 1
echo -1 >> 63
echo 1 << 2 < 5
echo 2 * 3 << 1
echo 1 << 2 << 3
echo 1 << 2 >> 1
echo 1 <<2
echo 1<< 2
echo 4 >>1
echo 8 >> 1 == 4
echo 1 ? 2 << 1 : 0
let l = [1]
echo l->len() << 3
echo 1 << 1 | echo 'next'
echo map([1,2,3], 'v:val << 1')
echo eval('3 << 2')
let x = 3 << 1
echo x
echo 1 << 1 2 << 2
echo !1 << 1
echo -1 << 1
" Errors: a left operand that is not a Number (no coercion, not even a Float,
" a Bool or a digit String), a right operand that is not one, a negative amount.
echo 1 << -1
echo '8' << 1
echo 8 << '1'
echo 1.0 << 1
echo 1 << 1.0
echo v:true << 1
echo 'a' . 1 << 2
echo [1] << 2
echo 1 << [2]
try
  echo 1 << -2
catch
  echo 'c2' v:exception
endtry
echo 2 >>? 1
" Legacy script has no compound form.
let y = 5
let y <<= 2
echo y
