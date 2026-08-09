" `{cond} ? {then}` with no `:` — vim raises E109 when the expression RUNS, so
" it is tagged with the ex-command and is catchable, and the truthy branch's
" side effects have already happened by then. vimlrs used to accept the form
" silently, which is worse than a wrong value: a script vim refuses would run.
"
" The second half pins the `Vim(cmd):` tag itself, which only shows up once a
" user function has been called: the tag names the command the *caller* is
" running, not the last one the callee ran.
let g:n = 0
function! Bump()
  let g:n += 1
  return 'v'
endfunction

try
  echo 1 ? Bump()
catch
  echo 'truthy' v:exception
endtry
echo 'n' g:n

try
  echo 0 ? Bump()
catch
  echo 'falsy' v:exception
endtry
echo 'n' g:n

try
  let g:z = Bump() + [][0]
catch
  echo 'after-call' v:exception
endtry

try
  echo Bump() . [][0]
catch
  echo 'after-call' v:exception
endtry

echo 1 ? 'uncaught'
echo 'still running'
