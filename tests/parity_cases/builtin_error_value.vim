" A builtin that reports an error still RETURNS a value, and the command around
" it still performs its action.
"
" c: `call_func()` returns FAIL only for an `FCERR_*` (unknown name, wrong
" argument count) — both raised by the dispatcher, never by the callee's body.
" Nothing an `f_*` reports with `emsg()` changes that, so `eval1()` succeeds and
" `ex_echo` prints (`vendor/eval.c:6146`) / `ex_let` assigns. An *evaluator*
" failure in the same position (`eval5`'s operand pre-check, an unknown function)
" is the opposite: the command is abandoned and the variable keeps its old value.
"
" Every probe below is `silent!` so the record is the VALUE, not the message; the
" un-silenced half is `echo str2nr('0x1f', 0)` at the end.

let g:r = 'UNSET'
silent! let g:r = str2nr('0x1f', 0)
echo 'str2nr    =' string(g:r)

" tv_get_string() on a List: E730 inside the body, rettv keeps its default.
let g:r = 'UNSET'
silent! let g:r = strlen([1])
echo 'strlen    =' string(g:r)

let g:r = 'UNSET'
silent! let g:r = toupper([1])
echo 'toupper   =' string(g:r)

let g:r = 'UNSET'
silent! let g:r = str2float([1])
echo 'str2float =' string(g:r)

" c:2873 `bool error = false;` … c:2880 `if (error) { len = 0; }` — a {start}
" that is not a Number gives the EMPTY string, not the whole of it.
let g:r = 'UNSET'
silent! let g:r = strpart('abc', [1])
echo 'strpart   =' string(g:r)

" E684 raised inside f_insert(): still a value.
let g:r = 'UNSET'
silent! let g:r = insert([], 1, 99)
echo 'insert    =' string(g:r)

" A user function's body errors; the function still returns what it returned.
function! Reporter()
  silent! call strlen([1])
  return 42
endfunction
let g:r = 'UNSET'
silent! let g:r = Reporter()
echo 'user-fn   =' string(g:r)

" c: `ex_return` — `eval0() == FAIL` returns through `do_return(…, NULL)`, i.e.
" with the value 0, NOT with the half-evaluated one.
function! Failer()
  return [1] . 'x'
endfunction
let g:r = 'UNSET'
silent! let g:r = Failer()
echo 'return-fail =' string(g:r)

" ── the other side of the line: an evaluator failure abandons the command ──

" eval5's left-operand pre-check for `.` — FAIL, so g:r is left alone.
let g:r = 'UNSET'
silent! let g:r = [1] . 'x'
echo 'concat    =' string(g:r)

" call_func() with no such function — FCERR_UNKNOWN, i.e. FAIL.
let g:r = 'UNSET'
silent! let g:r = nosuchfunction()
echo 'unknown   =' string(g:r)

" eval_index()'s own failure — FAIL.
let g:r = 'UNSET'
silent! let g:r = [][0]
echo 'index     =' string(g:r)

" ── unsilenced: the message AND the value, in that order ──
echo str2nr('0x1f', 0)

" ── inside a :try the error becomes an exception, and then the command IS
" abandoned: c: `eval_func` bails on `aborting()`, which `cause_errthrow` arms.
let g:r = 'UNSET'
try
  let g:r = str2nr('0x1f', 0)
catch
  echo 'caught' v:exception
endtry
echo 'try-let   =' string(g:r)
