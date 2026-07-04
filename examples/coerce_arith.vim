" coerce_arith.vim — implicit String->Number coercion in arithmetic, plus the
" sign/extremum builtins abs()/min()/max(). When a String is used in a numeric
" context Vim parses a leading integer literal: base prefixes 0x/0X/0b/0 are
" honoured, a fractional/scientific tail is dropped at the first non-digit, a
" non-numeric string is 0, and leading whitespace is NOT skipped (=> 0). The
" '.' concatenation operator instead stringifies both sides. Self-test: asserts
" into v:errors, throws if any failed.

" --- leading integer is parsed, the rest of the string is discarded
call assert_equal(8, '5' + 3)
call assert_equal(6, '5abc' + 1)
call assert_equal(12, '12abc34' + 0)
call assert_equal(3, '3.5' + 0)
call assert_equal(1, '1e3' + 0)

" --- a non-numeric (or whitespace-led) string coerces to 0
call assert_equal(5, 'abc' + 5)
call assert_equal(0, '  12  ' + 0)
call assert_equal(0, '.5' + 0)

" --- base prefixes: hex 0x/0X, binary 0b, and a leading 0 means OCTAL
call assert_equal(16, '0x10' + 0)
call assert_equal(31, '0X1F' + 0)
call assert_equal(5, '0b101' + 0)
call assert_equal(8, '010' + 0)

" --- a negative-signed string, and String on both sides of an operator
call assert_equal(-14, '-7' * 2)
call assert_equal(90, '99' - '9')

" --- '.' concatenates: both operands are stringified, no arithmetic
call assert_equal('53', '5' . 3)
call assert_equal('53', 5 . 3)
call assert_equal('820', 8 . 20)

" --- abs(): magnitude, preserving Int vs Float
call assert_equal(9, abs(-9))
call assert_equal(9, abs(9))
call assert_equal(3.5, abs(-3.5))

" --- max()/min() over a List; an empty List yields 0
call assert_equal(8, max([3, 8, 1]))
call assert_equal(-9, min([-2, -9, 4]))
call assert_equal(42, min([42]))
call assert_equal(0, max([]))
call assert_equal(0, min([]))

" --- max()/min() over a Dict operate on the VALUES, not the keys
call assert_equal(9, max({'a': 5, 'b': 9}))
call assert_equal(5, min({'a': 5, 'b': 9}))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'coerce_arith.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'coerce_arith.vim: all assertions passed'
