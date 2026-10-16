" When an error inside a call becomes an exception, the expression FAILs and
" the parse stops right after that call. A one-line `:catch` is reached only if
" what follows (past the `)` of enclosing calls) is the end of the argument or
" a `|`; with more expression text behind it the rest of the line is abandoned
" and the exception propagates to the caller of the script.
function! Thr() abort
  throw 'boom'
endfunction
function! Bad() abort
  return strlen([])
endfunction

function! s:Run(expr) abort
  let f = tempname()
  call writefile(['try | echo ' . a:expr . ' | catch | echo "caught" v:exception | endtry',
        \ 'echo "rest of file"'], f)
  echo '>>' a:expr
  try
    execute 'source' fnameescape(f)
  catch
    echo 'propagated' v:exception
  endtry
  call delete(f)
endfunction

" the call is the last thing: caught on the same line
call s:Run("Thr()")
call s:Run("Bad()")
call s:Run("1 + Thr()")
call s:Run("'a' . Bad()")
call s:Run("abs(Thr())")
call s:Run("abs(abs(Bad()))")
call s:Run("abs(1 + abs(Bad()))")
call s:Run("Thr() || 1")
call s:Run("max([1, 2])[0] + abs(Bad())")
" something else follows: the catch is never reached
call s:Run("Thr() + 1")
call s:Run("Bad() . 'x'")
call s:Run("(Thr())")
call s:Run("[Thr()]")
call s:Run("{'k': Bad()}")
call s:Run("Thr()[0]")
call s:Run("printf('%s %s', Bad(), 1)")
call s:Run("abs((Bad()))")
call s:Run("abs(Bad() + 1)")
" a throw inside a callee's own expression does not strand the caller's line
call s:Run("map([1], {-> Thr() + 1})")
call s:Run("map([1], {x -> abs(Bad())})")
" the value of a call that threw is not echoed
call s:Run("call('Thr', [])")
call s:Run("eval('Thr()')")
call s:Run("map([1], 'Thr()')")
call s:Run("substitute('a', 'a', '\\=Thr()', '')")
