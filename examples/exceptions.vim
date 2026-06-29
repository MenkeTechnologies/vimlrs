" exceptions.vim — :try/:catch/:finally and v:exception (ex_eval.c / vars.c).
"
" Demonstrates structured exception handling: :throw raises a value, :catch
" matches it by regex into v:exception, and :finally always runs — including
" while an exception propagates out through an inner block to an outer handler.
" Self-tests with the assert_* framework; throws (non-zero exit) on failure.
"
"   vimlrs examples/exceptions.vim

" ── a thrown value is caught and exposed as v:exception ──
let g:caught = ''
try
  throw 'boom'
catch /boom/
  let g:caught = v:exception
endtry
call assert_equal('boom', g:caught)

" ── :finally runs on the normal path ──
let g:trail = []
try
  call add(g:trail, 'body')
finally
  call add(g:trail, 'cleanup')
endtry
call assert_equal(['body', 'cleanup'], g:trail)

" ── :finally still runs while an exception unwinds to an outer :catch ──
let g:order = []
try
  try
    call add(g:order, 'inner-body')
    throw 'inner-fail'
  finally
    call add(g:order, 'inner-finally')
  endtry
catch /inner-fail/
  call add(g:order, 'outer-catch:' . v:exception)
endtry
call assert_equal(['inner-body', 'inner-finally', 'outer-catch:inner-fail'], g:order)

" ── a regex :catch picks the matching handler; a computed message round-trips ──
function! Classify(n) abort
  try
    if a:n < 0
      throw 'E-negative: ' . a:n
    endif
    throw 'ok: ' . a:n
  catch /^E-/
    return 'error'
  catch /^ok:/
    return 'fine'
  endtry
endfunction
call assert_equal('error', Classify(-3))
call assert_equal('fine', Classify(7))

" ── an unmatched :catch pattern lets the exception keep propagating ──
let g:reached = 0
try
  try
    throw 'specific'
  catch /will-not-match/
    let g:reached = 1
  endtry
catch /specific/
  " swallowed here
endtry
call assert_equal(0, g:reached)

" ── demo ──
echo 'Classify(-3) ->' Classify(-3)
echo 'Classify(7)  ->' Classify(7)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'exceptions.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
