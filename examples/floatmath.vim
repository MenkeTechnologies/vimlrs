" floatmath.vim — the floating-point math library (funcs.c: floor/ceil/round/
" trunc, the trig/hyperbolic/exp/log family, isnan/isinf). Transcendental
" results are compared as fixed-point integers (round(x * 10^k)) so the asserts
" stay exact regardless of the last ULP. Self-test: asserts into v:errors,
" throws if any failed.

" --- directed rounding: each rounds toward a different target, all keep a float
call assert_equal(3.0, floor(3.7))
call assert_equal(-4.0, floor(-3.1))
call assert_equal(4.0, ceil(3.2))
call assert_equal(-3.0, ceil(-3.9))
call assert_equal(3.0, round(2.5))
call assert_equal(-3.0, round(-2.5))
call assert_equal(3.0, trunc(3.9))
call assert_equal(-3.0, trunc(-3.7))

" --- float2nr truncates toward zero into an integer; abs on a float
call assert_equal(3, float2nr(3.99))
call assert_equal(-3, float2nr(-3.99))
call assert_equal(4.5, abs(-4.5))

" --- non-finite predicates
call assert_equal(1, isnan(0.0 / 0.0))
call assert_equal(0, isnan(1.0))
call assert_equal(1, isinf(1.0 / 0.0))
call assert_equal(-1, isinf(-1.0 / 0.0))
call assert_equal(0, isinf(1.0))

" --- exact float identities at the special angles
call assert_equal(1.0, cos(0.0))
call assert_equal(0.0, sin(0.0))
call assert_equal(1.0, exp(0.0))
call assert_equal(0.0, log(1.0))
call assert_equal(3.0, log10(1000.0))
call assert_equal(4.0, sqrt(16.0))
call assert_equal(1.0, cosh(0.0))
call assert_equal(0.0, sinh(0.0))
call assert_equal(0.0, tanh(0.0))
call assert_equal(0.0, tan(0.0))

" --- transcendentals compared as fixed-point integers (6 decimal places)
" asin(1) = acos(0) = pi/2; atan(1) = atan2(1,1) = pi/4
call assert_equal(1570796, float2nr(round(asin(1.0) * 1000000)))
call assert_equal(1570796, float2nr(round(acos(0.0) * 1000000)))
call assert_equal(785398, float2nr(round(atan(1.0) * 1000000)))
call assert_equal(785398, float2nr(round(atan2(1.0, 1.0) * 1000000)))
" e = exp(1); log is its inverse
call assert_equal(2718, float2nr(round(exp(1.0) * 1000)))
call assert_equal(1000000, float2nr(round(log(exp(1.0)) * 1000000)))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'floatmath.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'floatmath.vim: all assertions passed'
