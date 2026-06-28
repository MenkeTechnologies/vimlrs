" printf_exists.vim — printf() %S and *-from-argument width/precision, plus
" exists('*func') for builtins and user functions (funcs.c).
" %S formats a string (like %s); a `*` in the width or precision takes that value
" from the next argument (negative width left-justifies). exists('*name') reports
" whether a callable of that name is defined. Self-test into v:errors.

" --- %S renders a string; %* / %.* read width/precision from arguments
call assert_equal('abc', printf('%S', 'abc'))
call assert_equal('    3', printf('%*d', 5, 3))
call assert_equal('3    ', printf('%-*d', 5, 3))
call assert_equal('3.14', printf('%.*f', 2, 3.14159))
call assert_equal('    3.14', printf('%*.*f', 8, 2, 3.14159))

" --- exists('*name'): builtins are present, unknown names are not
call assert_equal(1, exists('*substitute'))
call assert_equal(1, exists('*printf'))
call assert_equal(0, exists('*no_such_function_xyz'))

" --- exists('*name') sees a user :function too
function! Greet(x)
  return 'hi ' . a:x
endfunction
call assert_equal(1, exists('*Greet'))
call assert_equal(0, exists('*Undefined_user_func'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'printf_exists.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'printf_exists.vim: all assertions passed'
