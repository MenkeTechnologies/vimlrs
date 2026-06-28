" error_output.vim — a runtime error in an expression aborts the command, so no
" spurious fallback value is produced (eval.c / ex_docmd.c semantics).
" At the CLI, `echo [1,2,3][10]` prints only the E684 message — never a trailing
" v:null; `echo printf('%d', 3.7)` prints only E805, never a trailing -1. Here we
" assert that those expressions raise their errors cleanly (assert_fails catches
" the error, keeping it off stderr) and that normal echo is unaffected.

" --- an out-of-range index raises E684 (and prints no fallback at the CLI)
call assert_fails('echo [1,2,3][10]', 'E684')

" --- using a Float where a Number is required raises E805
call assert_fails("echo printf('%d', 3.7)", 'E805')

" --- a key missing from a Dict raises E716
call assert_fails("echo {'a': 1}.b", 'E716')

" --- error-free echo still produces its normal output (captured via execute();
"     trim() keeps this independent of where execute() puts its newline)
call assert_equal('42', trim(execute('echo 42')))
call assert_equal('hi there', trim(execute('echo "hi" "there"')))
call assert_equal('2', trim(execute('echo [1,2,3][1]')))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'error_output.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'error_output.vim: all assertions passed'
