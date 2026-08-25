" `E116: Invalid arguments for function %s` — the SECOND diagnostic a call emits
" when one of its arguments failed to evaluate.
"
" c: get_func_tv calls call_func only when get_func_arguments returned OK
" (vendor/eval/userfunc.c:559-588); otherwise it reports E116 through
" emsg_funcname (c:492-500), which formats `%s` over the `name` POINTER. That
" pointer aims INTO the expression source and is not terminated at the end of the
" name, so the message carries everything from the name to the end of the eval
" buffer. `:call` is the exception: ex_call hands over a NUL-terminated copy.

function! Side(x)
  echo 'SIDE ' . a:x
  return a:x
endfunction

echo type([1] . '')
echo '--1'

" The tail runs to the end of the eval buffer — later :echo arguments included.
echo type([1] . '') 'TAIL'
echo '--2'

" ... and a following operator, and a closing bracket.
let g:z = type([1] . '') . 'X'
echo '--3'
echo [type([1] . '')]
echo '--4'

" Nested calls report one E116 each, innermost first.
echo type(type([1] . ''))
echo '--5'

" :call names the function and nothing else; a call nested in its arguments is
" still read from the source.
call type([1] . '')
echo '--6'
call type(type([1] . ''))
echo '--7'

" A call of a Funcref-valued VARIABLE names the function it points at.
let g:F = function('type')
echo g:F([1] . '')
echo '--8'

" A user function is named from the source like any other call.
echo Side([1] . '')
echo '--9'

" An undefined variable is a failed argument too.
echo type(g:nosuchvar)
echo '--10'

" An argument that merely REPORTS an error while still yielding a value does not
" fail eval1(), so there is no E116 and the call happens.
echo type(strlen([1]))
echo '--11'

" The argument count is checked only AFTER the arguments are evaluated, so their
" side effects happen and a failed argument pre-empts the count error.
echo strlen(Side(1), Side(2))
echo '--12'
echo len([1] . '', 'x')
echo '--13'

" No E116 for a method call: the C evaluated the base first and never entered
" get_func_arguments with a failing argument.
echo ([1] . '')->type()
echo '--14'
