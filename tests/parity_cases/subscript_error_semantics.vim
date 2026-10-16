" Errors inside a subscript: a Float or Blob-range problem is reported but the
" evaluation carries on with the value it has (`:echo` prints it, then the error
" is raised); a missing Dict key, a List index out of range and a Dict slice fail
" the expression. A failed Dict member read (`d.key`) abandons the rest of a
" one-line `:try`, and so does a failed subscript with more text behind it;
" `d['key']` and a Dict slice at the end of the argument reach the `:catch`.
function! s:Run(expr) abort
  let f = tempname()
  call writefile([
        \ 'let d = {"a": 1}',
        \ 'let l = [1, 2]',
        \ 'try | echo ' . a:expr . ' | catch | echo "caught" v:exception | endtry',
        \ 'echo "rest of file"'], f)
  echo '>>' a:expr
  try
    execute 'source' fnameescape(f)
  catch
    echo 'propagated' v:exception
  endtry
  call delete(f)
endfunction

" Float subscripts and bounds read as 0 after E805
call s:Run("l[1.5]")
call s:Run("'abc'[1.5]")
call s:Run("l[0:1.5]")
call s:Run("l[1.5:]")
call s:Run("0z0102[1.5]")
call s:Run("l[l[1.5]]")
call s:Run("abs(l[1.5])")
" a Blob index out of range leaves the Blob as the value
call s:Run("0z0102[2]")
call s:Run("0z0102[-3]")
" a List or Dict used as a subscript is stringified, and fails the expression
call s:Run("l[[1]]")
call s:Run("l[{}]")
call s:Run("'abc'[{}]")
" failures that end the argument are caught on the same line
call s:Run("d['b']")
call s:Run("l[5]")
call s:Run("d['a':]")
call s:Run("d[:1]")
" ... but not when more expression text follows
call s:Run("d['b'] . 'x'")
call s:Run("l[5] . 'x'")
call s:Run("[l[5]]")
" a failed `d.key` is hard wherever it sits
call s:Run("d.b")
call s:Run("d.b . 'x'")
call s:Run("[d.b]")
" v:null against a List, Dict, Blob or Float in == and !=
call s:Run("v:null == []")
call s:Run("[1] != v:null")
call s:Run("v:null == {}")
call s:Run("0z == v:null")
call s:Run("v:null == 0.0")
call s:Run("v:null != 1.5")
call s:Run("v:null < []")
