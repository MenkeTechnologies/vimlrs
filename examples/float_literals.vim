" float_literals.vim — Float literal grammar and rendering (eval.c).
" Neovim's Float literal is [0-9]+ '.' [0-9]+ with an optional [eE][+-]?[0-9]+
" exponent: the dot is mandatory, so '1.0e10' is a Float but '1e10' is the
" Number 1 followed by a name. string() of a Float is C printf %g (6 significant
" digits, C-style e+NN exponent), with '.0' appended when there is no '.'/'e' —
" matching Neovim (this differs from Vim 9.x's float printer). Self-test.

" --- well-formed float literals (dot required, optional exponent)
call assert_equal(3.14, str2float('3.14'))
call assert_equal(1500.0, 1.5e3)
call assert_equal(0.002, 2.0e-3)
call assert_equal(1, 1.0e10 == 10000000000.0)

" --- a dotless 'NeN' is NOT a float; '1e3' is the Number 1 then a name 'e3'
call assert_fails('echo string(1e3)', 'E15')

" --- string() renders a Float the Neovim way (C %g + trailing .0 when integral)
call assert_equal('1.0', string(1.0))
call assert_equal('0.3', string(0.1 + 0.2))
call assert_equal('3.14', string(3.14))
call assert_equal('1.5', string(1.5))
call assert_equal('-0.5', string(-0.5))

" --- IEEE negative zero keeps its sign (C %g), unlike positive zero
call assert_equal('0.0', string(0.0))
call assert_equal('-0.0', string(-0.0))
call assert_equal('-0.0', string(0.0 / -1.0))

" --- string() uses Vim's %g: fixed form in [1e-4, 1e7), exponent (eN/e-N) outside
call assert_equal('1000000.0', string(1000000.0))
call assert_equal('1.234568e8', string(123456789.0))
call assert_equal('1.0e-4', string(0.0001))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'float_literals.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'float_literals.vim: all assertions passed'
