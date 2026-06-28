" user_commands.vim — user-defined Ex commands: :command defines them and
" :Name invokes them (ported from Neovim's usercmd.c). Self-contained — a
" command is just a stored replacement, expanded and run on invocation.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- a plain command runs its replacement
command SetA let g:a = 'ran'
SetA
call assert_equal('ran', g:a)

" --- <args> substitutes the verbatim argument text
command -nargs=1 PutB let g:b = <args>
PutB 1 + 2
call assert_equal(3, g:b)

" --- <q-args> substitutes the arguments as a single quoted String
command -nargs=* QArgs let g:q = <q-args>
QArgs hello there
call assert_equal('hello there', g:q)
QArgs
call assert_equal('', g:q)

" --- <f-args> substitutes the arguments as comma-separated quoted values,
"     for passing straight into a function call
function! Collect(...) abort
  return join(a:000, '+')
endfunction
command -nargs=* FArgs let g:f = Collect(<f-args>)
FArgs 1 2 3
call assert_equal('1+2+3', g:f)

" --- <bang> expands to '!' when the command is invoked with a bang
command -bang Bang let g:bang = '<bang>'
Bang
call assert_equal('', g:bang)
Bang!
call assert_equal('!', g:bang)

" --- <lt> expands to a literal '<'
command Lt let g:lt = '<lt>nope>'
Lt
call assert_equal('<nope>', g:lt)

" --- redefining a command replaces it; a unique prefix also invokes it
command SetA let g:a = 'again'
SetA
call assert_equal('again', g:a)
Set
call assert_equal('again', g:a)

" --- :delcommand removes a command (it then no longer resolves)
command Gone let g:gone = 1
delcommand Gone

" --- the map()/filter() builtins and funcref calls are unaffected
call assert_equal([2, 4], map([1, 2], 'v:val * 2'))
let Up = {s -> toupper(s)}
call assert_equal('HI', Up('hi'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'user_commands.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'user_commands.vim: all assertions passed'
