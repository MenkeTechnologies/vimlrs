" delfunction.vim — `:delfunction[!] {name}` removes a user function from the
" registry (userfunc.c:ex_delfunction). Covers a plain global name, a script-
" local `s:` name, removal from inside an `:if` block (the case that fails when
" delfunction is not dispatched as a statement), the `g:`-prefixed and `!`
" (forceit) forms, and the abbreviated `:delf` spelling.
" Self-test: asserts into v:errors, throws if any failed.

function! Foo()
  return 1
endfunction
function! s:Bar()
  return 2
endfunction

" --- both are defined up front
call assert_equal(1, exists('*Foo'))
call assert_equal(1, exists('*s:Bar'))

" --- delfunction inside an `:if` block removes the plain global name
if 1
  delfunction Foo
endif
call assert_equal(0, exists('*Foo'))

" --- a script-local `s:` function is removed by its literal name
delfunction s:Bar
call assert_equal(0, exists('*s:Bar'))

" --- a plain global function addressed with an explicit `g:` and the `!`
" (forceit) form; `Foo` is global, so `g:Foo` names the same function
function! Qux()
  return 4
endfunction
call assert_equal(1, exists('*Qux'))
delfunction! g:Qux
call assert_equal(0, exists('*Qux'))
call assert_equal(0, exists('*g:Qux'))

" --- `:delfunction!` on a name that was never defined is silently tolerated
delfunction! NeverExisted
call assert_equal(0, exists('*NeverExisted'))

" --- the abbreviated `:delf` spelling also removes a function
function! Baz()
  return 3
endfunction
call assert_equal(1, exists('*Baz'))
delf Baz
call assert_equal(0, exists('*Baz'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'delfunction.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'delfunction.vim: all assertions passed'
