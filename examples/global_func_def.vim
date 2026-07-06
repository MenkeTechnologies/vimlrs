" global_func_def.vim — an explicit `g:` scope on a `:function` DEFINITION.
" `g:` is the default (global) namespace for a function, so `function! g:Baz()`
" registers under the bare name `Baz`: `exists('*Baz')` and `exists('*g:Baz')`
" both report it, a bare `call Baz()` reaches it, and `delfunction g:Baz`
" removes it (userfunc.c stores/looks up a `g:`-prefixed name under its bare
" form). Self-test: asserts into v:errors, throws if any failed.

function! g:Baz()
  return 42
endfunction

" --- found under the bare name and the explicit g: name alike
call assert_equal(1, exists('*Baz'))
call assert_equal(1, exists('*g:Baz'))

" --- callable by the bare name, by the explicit g: name, and via call()
call assert_equal(42, g:Baz())
call assert_equal(42, Baz())
call assert_equal(42, call('Baz', []))
call assert_equal(42, call('g:Baz', []))

" --- a funcref taken by the bare name resolves to the g:-defined body
let F = function('Baz')
call assert_equal(42, F())

" --- redefining with `function!` and the g: prefix overwrites in place
function! g:Baz()
  return 99
endfunction
call assert_equal(99, Baz())
call assert_equal(99, g:Baz())

" --- delfunction with the g: prefix removes the same function
delfunction g:Baz
call assert_equal(0, exists('*Baz'))
call assert_equal(0, exists('*g:Baz'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'global_func_def.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'global_func_def.vim: all assertions passed'
