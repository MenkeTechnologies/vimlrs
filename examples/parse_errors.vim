" parse_errors.vim — the message a failed PARSE reports, and the value a failed
" `eval()` answers. The vim-agreed half of this surface is pinned byte-for-byte
" by tests/parity_cases/parse_error_text.vim; what lives here is the half where
" vim and Neovim word the same diagnostic differently, plus the shape of the
" uncaught form. This port follows Neovim (README: the vendored C is the spec).
" Self-test: asserts into v:errors, throws at the end if anything failed.

function! s:Err(expr) abort
  try
    call eval(a:expr)
  catch
    return v:exception
  endtry
  return 'no error'
endfunction

" --- an unterminated quote. Neovim: "Missing quote". vim 9.2.0900 words the
"     same two codes "Missing single quote" / "Missing double quote".
call assert_equal("Vim(call):E115: Missing quote: 'abc", s:Err("'abc"))
call assert_equal('Vim(call):E114: Missing quote: "abc', s:Err('"abc'))

" --- an argument list that runs out. Neovim names the function and stops; vim
"     appends the unread source to it (`…for function f(1`).
call assert_equal('Vim(call):E116: Invalid arguments for function f', s:Err('f('))
call assert_equal('Vim(call):E116: Invalid arguments for function f', s:Err('f(1'))
call assert_equal('Vim(call):E116: Invalid arguments for function f', s:Err('f(1,'))

" --- junk after a number inside a call is E15, NOT E116: the argument itself
"     is what failed. (`1e3` is not a float literal in either engine — a float
"     needs the fraction digits, so this is `1` followed by the name `e3`.)
call assert_match('^Vim(call):E15: Invalid expression: ', s:Err('string(1e3)'))

" --- a caught parse failure leaves the assignment target untouched: the `:let`
"     never completes, so the variable keeps whatever it had. Measured
"     identically in vim 9.2.0900 and nvim 0.12.4 — the Number 0 that `f_eval`
"     writes into its rettv is not what the caller sees here.
let s:v = 'unset'
let s:caught = ''
try
  let s:v = eval('((1)')
catch
  let s:caught = v:exception
endtry
call assert_equal("Vim(let):E110: Missing ')'", s:caught)
call assert_equal('unset', s:v)

" --- the same for a failure with nothing specific to say, where the only
"     message is E15 over the whole expression.
let s:v = 'unset'
let s:caught = ''
try
  let s:v = eval('1 +')
catch
  let s:caught = v:exception
endtry
call assert_equal('Vim(let):E15: Invalid expression: "1 +"', s:caught)
call assert_equal('unset', s:v)

" --- a SUCCESSFUL eval() with trailing text is a different failure: the value
"     is the leading expression and the message is E488.
call assert_equal('Vim(call):E488: Trailing characters: ,2', s:Err('1,2'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'parse_errors.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'parse_errors.vim: all assertions passed'
