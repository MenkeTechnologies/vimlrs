" math_extra.vim — the C-math builtins not covered by floatmath.vim: pow() and
" fmod(). Both always return a Float. pow(b,e) == b**e; fmod(a,b) is the IEEE
" remainder a - trunc(a/b)*b, so it keeps the sign of the dividend. Exact
" identities are asserted directly; a non-terminating remainder is compared as a
" fixed-point integer. Self-test: asserts into v:errors, throws if any failed.

" --- pow(): exact powers stay exact floats
call assert_equal(1024.0, pow(2.0, 10.0))
call assert_equal(3.0, pow(9.0, 0.5))
call assert_equal(0.5, pow(2.0, -1.0))
call assert_equal(1.0, pow(0.0, 0.0))
call assert_equal(1000, float2nr(pow(10.0, 3.0)))

" --- fmod(): remainder keeps the dividend's sign
call assert_equal(1.0, fmod(10.0, 3.0))
call assert_equal(-1.0, fmod(-10.0, 3.0))
call assert_equal(1.5, fmod(7.5, 2.0))
call assert_equal(0.0, fmod(8.0, 2.0))

" --- a non-terminating remainder compared as a fixed-point integer (6 places)
call assert_equal(1300000, float2nr(round(fmod(5.3, 2.0) * 1000000)))

" --- pow inverts a root: pow(pow(x, y), 1/y) == x for x > 0
call assert_equal(2000, float2nr(round(pow(pow(2.0, 3.0), 1.0 / 3.0) * 1000)))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'math_extra.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'math_extra.vim: all assertions passed'
