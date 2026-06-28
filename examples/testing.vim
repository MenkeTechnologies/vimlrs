" testing.vim — writing unit tests with vimlrs's built-in assert framework.
"
" Demonstrates the value-asserts (assert_equal/assert_true/assert_match/
" assert_notmatch/assert_inrange) plus the error-path testers: assert_fails
" (a command must produce an error, optionally matching an error code) and
" assert_exception (inside :catch, the thrown text must match). Like every
" example here it self-checks and exits non-zero on failure, so CI catches
" regressions.
"
"   vimlrs examples/testing.vim

" A tiny function under test: parse 'KEY=VALUE', throwing on malformed input.
function! ParseKV(s) abort
  let m = matchlist(a:s, '^\(\w\+\)=\(.*\)$')
  if empty(m)
    throw 'E0: not KEY=VALUE: ' . a:s
  endif
  return [m[1], m[2]]
endfunction

" ── happy path ──
call assert_equal(['name', 'ada'], ParseKV('name=ada'))
call assert_equal(['x', ''], ParseKV('x='))
call assert_true(len(ParseKV('a=b')) == 2)

" ── error path via :try/:catch + assert_exception ──
try
  call ParseKV('garbage')
  call assert_report('ParseKV should have thrown on bad input')
catch
  call assert_exception('E0:')
endtry

" ── error path via assert_fails (the command must error) ──
call assert_fails("call ParseKV('nope')", 'E0:')
call assert_fails('call no_such_function()', 'E117:')
call assert_fails('let x = [] + 1')

" ── value asserts ──
call assert_inrange(1, 10, 7)
call assert_match('\d\+', 'abc123')
call assert_notmatch('^\d', 'abc123')

" ── demo ──
echo 'ParseKV(name=ada) ->' ParseKV('name=ada')
echo 'assertions run, v:errors empty?' empty(v:errors)

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: testing assertions passed'
