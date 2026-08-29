" The `Vim(cmd):` tag on an error raised inside execute() names the command that
" was RUNNING when it failed, not the one that called execute().
"
" The tag is emitted per statement, but only for programs the compiler sees as
" using exceptions. An execute() string is compiled as its own program, so one
" containing no :try emitted no tag at all and the error kept whatever the
" ENCLOSING statement had set -- `Vim(let)` here, from the `:let` that captured
" the result. The caller's :try is what observes it, so a nested program always
" has to carry it.
try
  let s:r = execute('echo {}+1')
catch
  echo v:exception
endtry
try
  let s:r = execute('echo [] + 1')
catch
  echo v:exception
endtry
try
  let s:r = execute('echo undefined_fn_xyz()')
catch
  echo v:exception
endtry
" A nested :let really is `let`, and must stay so.
try
  let s:r = execute('let x = {}+1')
catch
  echo v:exception
endtry
" No error at all: execute() returns the captured output.
echo execute('echo 1')
