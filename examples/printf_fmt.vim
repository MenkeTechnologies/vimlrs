" printf_fmt.vim — printf() conversion specifiers (strings.c/vim_snprintf):
" %d/%x/%X/%o/%b integer radices, flags (0, -, +, #), field width and precision,
" %e/%f/%g float forms, %s/%c, the %% literal, '*' width from an argument, and
" N$ positional arguments. printf returns a String. Self-test: asserts into
" v:errors, throws if any failed.

" --- %d and integer flags/width
call assert_equal('42', printf('%d', 42))
call assert_equal('-7', printf('%d', -7))
call assert_equal('00042', printf('%05d', 42))
call assert_equal('42   |', printf('%-5d|', 42))
call assert_equal('+42', printf('%+d', 42))
call assert_equal('   42', printf('%*d', 5, 42))

" --- radices: hex (lower/upper, with #), octal, binary
call assert_equal('ff', printf('%x', 255))
call assert_equal('FF', printf('%X', 255))
call assert_equal('0xff', printf('%#x', 255))
call assert_equal('10', printf('%o', 8))
call assert_equal('101', printf('%b', 5))

" --- floats: fixed, scientific, and %g (shortest)
call assert_equal('3.14', printf('%.2f', 3.14159))
call assert_equal('    3.14|', printf('%8.2f|', 3.14159))
call assert_equal('1.234568e+04', printf('%e', 12345.678))
call assert_equal('1.0e-4', printf('%g', 0.0001))

" --- strings and chars, with precision-as-truncation
call assert_equal('a-b', printf('%s-%s', 'a', 'b'))
call assert_equal('hel', printf('%.3s', 'hello'))
call assert_equal('A', printf('%c', 65))

" --- the literal percent and positional (N$) arguments
call assert_equal('%', printf('%%'))
call assert_equal('b a', printf('%2$s %1$s', 'a', 'b'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'printf_fmt.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'printf_fmt.vim: all assertions passed'
