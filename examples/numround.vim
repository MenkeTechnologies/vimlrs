" numround.vim — magnitude and rounding: abs/ceil/floor/trunc/round/float2nr
" (float.c/eval.c). abs() strips the sign of a Number or Float; ceil/floor/trunc
" return a Float rounded toward +inf / -inf / zero; round() rounds half AWAY from
" zero (so 2.5 -> 3.0, -2.5 -> -3.0); float2nr() truncates a Float toward zero
" into a Number. Negatives and exact halves are the interesting cases here.
" Self-test: asserts into v:errors, throws if any failed.

" --- abs(): sign strip for Number and Float
call assert_equal(5, abs(-5))
call assert_equal(5, abs(5))
call assert_equal(100, abs(-100))
call assert_equal(2147483648, abs(-2147483648))
call assert_equal(0, abs(0))
" float2nr(round(abs(x)*1e6)) fixed-point compare for Float results
call assert_equal(3500000, float2nr(round(abs(-3.5) * 1000000.0)))
call assert_equal(3500000, float2nr(round(abs(3.5) * 1000000.0)))

" --- ceil(): toward +inf (returns Float)
call assert_equal(3000000, float2nr(round(ceil(2.1) * 1000000.0)))
call assert_equal(-2000000, float2nr(round(ceil(-2.1) * 1000000.0)))
call assert_equal(2000000, float2nr(round(ceil(2.0) * 1000000.0)))
call assert_equal(-2000000, float2nr(round(ceil(-2.9) * 1000000.0)))

" --- floor(): toward -inf (returns Float)
call assert_equal(2000000, float2nr(round(floor(2.9) * 1000000.0)))
call assert_equal(-3000000, float2nr(round(floor(-2.1) * 1000000.0)))
call assert_equal(2000000, float2nr(round(floor(2.0) * 1000000.0)))

" --- trunc(): toward zero (returns Float)
call assert_equal(2000000, float2nr(round(trunc(2.9) * 1000000.0)))
call assert_equal(-2000000, float2nr(round(trunc(-2.9) * 1000000.0)))
call assert_equal(-2000000, float2nr(round(trunc(-2.1) * 1000000.0)))

" --- round(): half away from zero (returns Float)
call assert_equal(3000000, float2nr(round(2.5) * 1000000))
call assert_equal(-3000000, float2nr(round(-2.5) * 1000000))
call assert_equal(2000000, float2nr(round(2.4) * 1000000))
call assert_equal(-2000000, float2nr(round(-2.4) * 1000000))
call assert_equal(1000000, float2nr(round(0.5) * 1000000))
call assert_equal(-1000000, float2nr(round(-0.5) * 1000000))

" --- float2nr(): truncate toward zero into a Number
call assert_equal(3, float2nr(3.7))
call assert_equal(-3, float2nr(-3.7))
call assert_equal(3, float2nr(3.2))
call assert_equal(-2, float2nr(-2.999))
call assert_equal(1000000000, float2nr(1000000000.5))
call assert_equal(0, float2nr(0.0))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'numround.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'numround.vim: all assertions passed'
