" numbers.vim — float rendering (%g) and max()/min() over lists and dicts.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- float string() rendering matches Vim's printf("%g") (6 significant digits)
call assert_equal('0.3', string(0.1 + 0.2))
call assert_equal('1.0', string(1.0))
call assert_equal('1.5', string(1.5))
call assert_equal('0.333333', string(1.0 / 3.0))
call assert_equal('100.0', string(100.0))
call assert_equal('-2.5', string(-2.5))
call assert_equal('0.0', string(0.0))
call assert_equal('3.141593', string(3.14159265))

" --- json_encode uses the same float rendering
call assert_equal('{"x":0.5}', json_encode({'x': 0.5}))

" --- str2float parses the leading float, ignoring trailing garbage (strtod)
call assert_equal(3.14, str2float('3.14'))
call assert_equal(3.14, str2float('3.14abc'))
call assert_equal(-2500.0, str2float('  -2.5e3xyz'))
call assert_equal(0.5, str2float('.5'))
call assert_equal(0.0, str2float('abc'))

" --- max()/min() over a list
call assert_equal(8, max([3, 8, 1, 5]))
call assert_equal(1, min([3, 8, 1, 5]))
call assert_equal(0, max([]))
call assert_equal(0, min([]))

" --- max()/min() over a dict (values, not keys) — the bug that was fixed
call assert_equal(8, max({'a': 5, 'b': 2, 'c': 8}))
call assert_equal(2, min({'a': 5, 'b': 2, 'c': 8}))

" --- integer literals: octal (leading 0), hex, binary
call assert_equal(8, 010)
call assert_equal(511, 0777)
call assert_equal(129, 0129)
call assert_equal(255, 0xff)
call assert_equal(5, 0b101)
call assert_equal(15, 0o17)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'numbers.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'numbers.vim: all assertions passed'
