" `:try` inside a `:function … abort`, where two rules meet.
"
" c: `abort` is FC_ABORT (vendor/eval/userfunc.h:20), which makes do_cmdline
" skip the did_emsg reset it otherwise performs after every command of a
" function body (ex_docmd.c:647-651), so the flag survives and ea.skip
" (c:2027-2031) drops the rest of the body. A `:try` around the failing command
" changes that entirely: cause_errthrow (vendor/ex_eval.c:158) converts the
" error into an EXCEPTION instead, and `:catch` resets did_emsg
" (vendor/ex_eval.c:116) — so a caught error leaves an abort function running.
"
" NOT probed: the same shapes under `:silent!`. Both engines stop the sourced
" script there and this port carries on; see BUGS.md.

" A caught error inside an abort function is not an abort at all.
function! A() abort
  try
    echo [1] . 'x'
    echo 'NEVER-A'
  catch
    echo '  caught ' . v:exception
  endtry
  echo '  after-try A'
  return 'A-done'
endfunction
echo A()
echo '--1'

" A catch that matches by PATTERN, and one that does not.
function! B() abort
  try
    echo [1] . 'x'
  catch /E999/
    echo '  NEVER-B-catch'
  endtry
  echo '  NEVER-after-B'
  return 'B-done'
endfunction
try
  echo B()
catch
  echo '  propagated ' . v:exception
endtry
echo '--2'

" The error crosses a CALL boundary: the callee is abort, the caller catches.
function! C() abort
  echo [1] . 'x'
  echo '  NEVER-C'
  return 'C-done'
endfunction
function! D() abort
  try
    call C()
    echo '  NEVER-after-call'
  catch
    echo '  caught in D ' . v:exception
  endtry
  return 'D-done'
endfunction
echo D()
echo '--3'

" A caught error inside a loop inside an abort function: the loop keeps going.
function! E() abort
  let l:n = 0
  while l:n < 3
    let l:n += 1
    try
      echo [1] . 'x'
    catch
      echo '  caught ' . l:n
    endtry
  endwhile
  return 'E-done n=' . l:n
endfunction
echo E()
echo '--4'

" An error AFTER a completed try/catch still aborts the body.
function! F() abort
  try
    throw 'boom'
  catch /boom/
    echo '  caught boom'
  endtry
  echo [1] . 'x'
  echo '  NEVER-F'
  return 'F-done'
endfunction
try
  echo F()
catch
  echo '  F propagated ' . v:exception
endtry
echo '--5'

" A :finally always runs, and the error carries on out of the function.
function! G() abort
  try
    echo [1] . 'x'
  finally
    echo '  finally G'
  endtry
  echo '  NEVER-after-G'
  return 'G-done'
endfunction
try
  echo G()
catch
  echo '  G propagated ' . v:exception
endtry
echo '--6'

" A NON-abort function is the control: the body carries on past the error.
function! H()
  echo [1] . 'x'
  echo '  H carries on'
  return 'H-done'
endfunction
echo H()
echo '--7'
