" signs.vim — sign_define()/sign_getdefined()/sign_undefine(), the sign
" definition table ported from Neovim's sign.c. Standalone it is an in-memory
" name → attributes map (no buffers to place signs in).
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- nothing is defined initially
call assert_equal([], sign_getdefined())

" --- sign_define() registers a sign with its attribute Dict
call sign_define('err', {'text': 'E>', 'texthl': 'Error'})
call sign_define('warn', {'text': 'W>'})

" --- sign_getdefined() lists every sign, name merged with its attributes
call assert_equal([{'name': 'err', 'text': 'E>', 'texthl': 'Error'}, {'name': 'warn', 'text': 'W>'}], sign_getdefined())

" --- sign_getdefined({name}) returns just that sign
call assert_equal([{'name': 'err', 'text': 'E>', 'texthl': 'Error'}], sign_getdefined('err'))

" --- an unknown name yields an empty list
call assert_equal([], sign_getdefined('nope'))

" --- redefining replaces the attributes
call sign_define('warn', {'text': '!!'})
call assert_equal([{'name': 'warn', 'text': '!!'}], sign_getdefined('warn'))

" --- sign_undefine({name}) removes one sign
call sign_undefine('err')
call assert_equal([{'name': 'warn', 'text': '!!'}], sign_getdefined())

" --- sign_undefine() with no argument clears them all
call sign_undefine()
call assert_equal([], sign_getdefined())

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'signs.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'signs.vim: all assertions passed'
