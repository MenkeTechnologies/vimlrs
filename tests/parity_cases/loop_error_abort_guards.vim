" The three things that must NOT trigger the block skip, and the one function
" attribute that must.
"
" c: `:silent!` raises `emsg_silent`, and emsg_multiline returns at
" vendor/message.c:817-846 BEFORE `did_emsg++` at c:870 — a silenced error does
" not skip anything. Inside a `:try`, cause_errthrow (c:158) converts the error
" into an exception instead, and `:catch` resets did_emsg
" (vendor/ex_eval.c:116). A plain function body is its own `do_cmdline`, which
" resets did_emsg after every command it runs unless the function is `abort`
" (ex_docmd.c:647-651).

echo '-- 1. :silent! does not abort the loop'
let g:n = 0
while g:n < 3
  let g:n += 1
  silent! echo [1] . 'x'
  echo 'still here n=' . g:n
endwhile
echo 'n=' . g:n . ' errmsg=' . v:errmsg

echo '-- 2. a caught error does not abort the loop'
let g:n = 0
while g:n < 3
  let g:n += 1
  try
    echo [1] . 'x'
  catch
    echo 'caught ' . v:exception
  endtry
endwhile
echo 'n=' . g:n

echo '-- 3. an UNCAUGHT error inside a :try around the loop still stops the loop'
let g:n = 0
try
  while g:n < 3
    let g:n += 1
    echo [1] . 'x'
  endwhile
catch
  echo 'caught ' . v:exception
endtry
echo 'n=' . g:n

echo '-- 4. an error inside a plain function does not abort the caller'
function! Plain()
  echo [1] . 'x'
  echo 'the plain body carries on'
endfunction
let g:n = 0
while g:n < 3
  let g:n += 1
  call Plain()
endwhile
echo 'n=' . g:n

echo '-- 5. ... but an `abort` function stops its own body AND the caller'
function! Aborting() abort
  echo [1] . 'x'
  echo 'NEVER'
endfunction
let g:n = 0
while g:n < 3
  let g:n += 1
  call Aborting()
  echo 'NEVER-CALLER'
endwhile
echo 'n=' . g:n

echo '-- 6. a lambda is not a do_cmdline: its error DOES stop the caller'
let g:L = {x -> x . [1]}
let g:n = 0
while g:n < 3
  let g:n += 1
  call g:L(1)
  echo 'NEVER-LAMBDA'
endwhile
echo 'n=' . g:n
