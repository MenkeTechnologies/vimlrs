" funcref_builtin.vim — the funcref() builtin over USER functions. funcref()
" makes a Funcref just like function(), and (like function()) accepts a leading
" argument List to pre-bind into a Partial. The resulting Funcref can be called
" directly with (args), passed to call(), or stored in a List/Dict element and
" invoked through a bracket index. Self-test: asserts into v:errors, throws if
" any failed.

function! Sq(n) abort
  return a:n * a:n
endfunction

function! Add(a, b) abort
  return a:a + a:b
endfunction

" --- funcref('name') resolves to a callable Funcref (invoke it directly)
let R = funcref('Sq')
call assert_equal(49, R(7))
call assert_equal(0, R(0))

" --- funcref() and call() interoperate
call assert_equal(64, call(funcref('Sq'), [8]))
call assert_equal(30, call(funcref('Add'), [10, 20]))

" --- a leading List pre-binds arguments into a Partial; remaining args follow
let P = funcref('Add', [100])
call assert_equal(105, P(5))
call assert_equal(101, P(1))

" --- funcref() gives the SAME result as function() for a user function
call assert_equal(function('Sq')(9), funcref('Sq')(9))

" --- a Funcref stored in a List element, invoked via a bracket index
let fns = [funcref('Sq'), funcref('Add')]
call assert_equal(36, fns[0](6))
call assert_equal(30, fns[1](10, 20))

" --- a Funcref stored in a Dict element, invoked via a bracket index
let d = {'sq': funcref('Sq'), 'add': funcref('Add')}
call assert_equal(81, d['sq'](9))
call assert_equal(7, d['add'](3, 4))

" --- funcref() reports Funcref type (type 2 in Vim's type() numbering)
call assert_equal(type(function('Sq')), type(funcref('Sq')))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'funcref_builtin.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'funcref_builtin.vim: all assertions passed'
