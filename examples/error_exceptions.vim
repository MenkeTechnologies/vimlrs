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

" --- a command whose expression errored is ABANDONED: its effect does not happen.
"     A failed `:let` therefore leaves the variable exactly as it was, rather than
"     storing whatever value the evaluator recovered with.
let s:keep = 'orig'
silent! let s:keep = [1] . 'x'
call assert_equal('orig', s:keep)

" --- an error abandons the REST OF THE COMMAND LINE: the `|`-separated commands
"     after the failing one do not run, and execution resumes at the next line
let s:ran = 0
silent! echo [1] . 'x' | let s:ran = 1
call assert_equal(0, s:ran)

" --- ...but a line whose commands all succeed runs every one of them
let s:n = 0
let s:n = 1 | let s:n = s:n + 1
call assert_equal(2, s:n)

" Run {cmd} inside a ONE-LINE try and report whether its `:catch` saw the error.
" The inner `:execute` builds the one-liner so the whole thing stays on one line.
func! s:CatchInline(cmd) abort
  let s:got = 'escaped'
  try
    execute 'try | ' . a:cmd . ' | catch | let s:got = "caught" | endtry'
  catch
    " the error escaped the inline :catch and reached this (multi-line) one
  endtry
  return s:got
endfunc

" --- a one-line `try | … | catch | … | endtry` DOES catch an ordinary runtime error
"     (and a `:throw`), exactly like a multi-line one
let s:inline = ''
try | call nosuchfn() | catch | let s:inline = v:exception | endtry
call assert_match('E117', s:inline)

let s:inline_throw = ''
try | throw 'boom' | catch | let s:inline_throw = v:exception | endtry
call assert_equal('boom', s:inline_throw)

" --- ...but NOT an error raised by the expression evaluator's OWN type checks.
"     Coercing a condition, indexing something unindexable, and the `eval5` operand
"     pre-check all make Vim's eval1() return FAIL, and a command whose argument
"     failed to evaluate takes the whole command line with it — so the `:catch` on
"     that line never runs. An error raised *inside a called builtin* (E117 above,
"     E684 from insert()) does not fail the evaluator and IS caught. A multi-line
"     `:try`, whose `:catch` is on another line, catches both; see the block above.
call assert_equal('caught', s:CatchInline('call nosuchfn()')) " builtin → soft
call assert_equal('caught', s:CatchInline('echo insert([1],{},100000)')) " builtin → soft
call assert_equal('caught', s:CatchInline('echo deepcopy({})[2]')) " E716 → soft
call assert_equal('escaped', s:CatchInline('echo [1] . "x"')) " eval5 pre-check → HARD
call assert_equal('escaped', s:CatchInline('echo 0z11 - 1')) " eval5 pre-check → HARD
call assert_equal('escaped', s:CatchInline('echo log10(-3.25)[-5:0]')) " unindexable → HARD
call assert_equal('escaped', s:CatchInline('echo (sort([3,1,2],"n") ? v:true : [1])')) " condition → HARD
let s:esc = ''
try
  try | echo [1] . 'x' | catch | call assert_report('a hard failure must not be caught inline') | endtry
catch
  let s:esc = v:exception
endtry
call assert_match('E730', s:esc)

" --- and a hard failure abandons the rest of the line even under `:silent!`, while an
"     ordinary silenced error lets the line continue
let s:hard = 'not-run'
silent! echo [1] . 'x' | let s:hard = 'ran'
call assert_equal('not-run', s:hard)

let s:soft = 'not-run'
silent! call nosuchfn() | let s:soft = 'ran'
call assert_equal('ran', s:soft)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'error_exceptions.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
