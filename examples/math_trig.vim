" math_trig.vim — the transcendental float builtins (math.c: sqrt, exp, log,
" log10, the trig sin/cos/tan + inverse asin/acos/atan/atan2, and the
" hyperbolic sinh/cosh/tanh). Each returns a Float; to compare exactly across
" engines every case is reduced to an integer via float2nr(round(x*1000000)),
" the fixed-point idiom (6 fractional digits). Inverse/forward identities
" (sin(asin x)=x, tan(atan x)=x) pin the round-trips. Self-test: throws on fail.

" --- sqrt(): principal square root; 0 and a Pythagorean triple
call assert_equal(0, float2nr(round(sqrt(0.0) * 1000000)))
call assert_equal(1414214, float2nr(round(sqrt(2.0) * 1000000)))
call assert_equal(5000000, float2nr(round(sqrt(pow(3.0, 2.0) + pow(4.0, 2.0)) * 1000000)))

" --- exp()/log(): natural exp and its inverse (log(exp x)=x, exp(0)=1, log(1)=0)
call assert_equal(2718282, float2nr(round(exp(1.0) * 1000000)))
call assert_equal(1000000, float2nr(round(exp(0.0) * 1000000)))
call assert_equal(1000000, float2nr(round(log(exp(1.0)) * 1000000)))
call assert_equal(0, float2nr(round(log(1.0) * 1000000)))

" --- log10(): base-10 log (log10(1000)=3)
call assert_equal(3000000, float2nr(round(log10(1000.0) * 1000000)))

" --- sin()/cos()/tan() at 0, plus tan(atan x)=x round-trip
call assert_equal(0, float2nr(round(sin(0.0) * 1000000)))
call assert_equal(1000000, float2nr(round(cos(0.0) * 1000000)))
call assert_equal(0, float2nr(round(tan(0.0) * 1000000)))
call assert_equal(2500000, float2nr(round(tan(atan(2.5)) * 1000000)))

" --- asin()/acos()/atan(): inverse trig; asin(1)=pi/2, acos(1)=0, atan(1)=pi/4
call assert_equal(1570796, float2nr(round(asin(1.0) * 1000000)))
call assert_equal(0, float2nr(round(acos(1.0) * 1000000)))
call assert_equal(785398, float2nr(round(atan(1.0) * 1000000)))
call assert_equal(500000, float2nr(round(sin(asin(0.5)) * 1000000)))
call assert_equal(250000, float2nr(round(cos(acos(0.25)) * 1000000)))

" --- atan2(): two-argument arctangent respects the quadrant (pi/4, -3pi/4)
call assert_equal(785398, float2nr(round(atan2(1.0, 1.0) * 1000000)))
call assert_equal(-2356194, float2nr(round(atan2(-1.0, -1.0) * 1000000)))

" --- sinh()/cosh()/tanh(): hyperbolic; sinh(0)=0, cosh(0)=1, tanh saturates
call assert_equal(0, float2nr(round(sinh(0.0) * 1000000)))
call assert_equal(1175201, float2nr(round(sinh(1.0) * 1000000)))
call assert_equal(1000000, float2nr(round(cosh(0.0) * 1000000)))
call assert_equal(761594, float2nr(round(tanh(1.0) * 1000000)))

" --- every result is a Float
call assert_equal(v:t_float, type(sqrt(4.0)))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'math_trig.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'math_trig.vim: all assertions passed'
