" error_exceptions.vim — a runtime error inside `:try` is a catchable exception.
"
" Vim does not merely *print* an E-error: inside a `:try` it converts it into an
" exception (`emsg` → `cause_errthrow`, ex_eval.c), aborts the rest of the
" protected block, and lets `:catch` see it as `Vim(<cmd>):E<nnn>: …`. Plugins
" lean on this constantly (`try | call Foo() | catch /E117/ | endtry`), so it is
" compat-floor behavior, not a nicety.
"
" Outside a `:try` the error is printed and the script keeps going, as before.
" Self-tests with assert_*; exits non-zero on any failure.
"
"   vimlrs examples/error_exceptions.vim

" --- an error in the protected block is caught, and the block is abandoned
let s:reached = 0
try
  echo [1] . 'x'
  let s:reached = 1
catch
  let s:caught = v:exception
endtry
call assert_equal(0, s:reached)
call assert_match('E730', s:caught)

" --- the exception is tagged with the ex-command that raised it
call assert_match('^Vim(echo):', s:caught)
try
  call nosuchfn()
catch
  call assert_match('^Vim(call):E117:', v:exception)
endtry

" --- `:catch /pat/` matches on the error number, the usual plugin idiom
try
  echo [1] . 'x'
  call assert_report('E730 should have been thrown')
catch /E730/
  call assert_true(v:true)
endtry

" --- a non-matching `:catch` does not swallow it; an outer `:try` sees it
let s:outer = ''
try
  try
    echo {'a': 1} . 'x'
  catch /E999/
    call assert_report('E999 must not match E731')
  endtry
catch
  let s:outer = v:exception
endtry
call assert_match('E731', s:outer)

" --- the FIRST error wins: `eval5` type-checks the left operand of `-` before it
"     even evaluates the right one, so the Blob (E974) is reported, not the
"     failing remove() on the right
try
  let s:x = 0z - remove({'a': 1}, 'nokey')
catch
  call assert_match('E974', v:exception)
endtry

" --- `:finally` still runs, and a caught error does not leak into it
let s:fin = 0
try
  echo [1] . 'x'
catch
  " swallowed
finally
  let s:fin = 1
endtry
call assert_equal(1, s:fin)

" --- `:silent!` suppresses the error message; the command still fails, and the
"     script neither reports it nor exits non-zero because of it
let s:after = 0
silent! call nosuchfn()
let s:after = 1
call assert_equal(1, s:after)

" --- outside a `:try` an error is not thrown, so execution simply continues
"     (assert_fails runs the command and captures the error it raises)
call assert_fails('echo [1] . "x"', 'E730')

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'error_exceptions.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
