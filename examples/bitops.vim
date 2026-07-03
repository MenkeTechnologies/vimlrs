" bitops.vim — the bitwise builtins (funcs.c: and/or/xor/invert). Vim integers
" are signed 64-bit two's-complement, so invert(x) == -x - 1 and invert(-1) == 0.
" Self-test: asserts into v:errors, throws if any failed.

" --- and(): bit intersection
call assert_equal(8, and(12, 10))
call assert_equal(0, and(0, 255))
call assert_equal(255, and(-1, 255))
call assert_equal(3840, and(0xFF00, 0x0FF0))

" --- or(): bit union
call assert_equal(14, or(12, 10))
call assert_equal(0, or(0, 0))
call assert_equal(255, or(0x0F, 0xF0))

" --- xor(): bit difference; x xor x == 0, x xor 0 == x
call assert_equal(6, xor(12, 10))
call assert_equal(0, xor(255, 255))
call assert_equal(12, xor(0, 12))

" --- invert(): two's-complement flip; invert(x) == -x - 1
call assert_equal(-1, invert(0))
call assert_equal(-6, invert(5))
call assert_equal(0, invert(-1))

" --- identities: and(x,x)==x, or(x,0)==x, invert(invert(x))==x
let x = 0x2A3B
call assert_equal(x, and(x, x))
call assert_equal(x, or(x, 0))
call assert_equal(x, invert(invert(x)))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'bitops.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'bitops.vim: all assertions passed'
