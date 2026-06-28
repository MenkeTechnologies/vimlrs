" execute_capture.vim — execute() captures command output, Vim-style (funcs.c).
" execute({command}) runs the command(s) with output redirected and returns it
" as a string. Each :echo is *preceded* by a newline (so a single 'echo 5'
" yields "\n5"); :echon adds no newline. A List of commands runs in order.
" Self-test into v:errors.

" --- a single :echo is captured with a leading newline
call assert_equal("\n5", execute('echo 5'))
call assert_equal("\n42", execute('echo 41 + 1'))

" --- :echon contributes no newline
call assert_equal('5', execute('echon 5'))

" --- a List of commands runs in order, each :echo adding its leading newline
call assert_equal("\n1\n2", execute(['echo 1', 'echo 2']))

" --- the result is an ordinary string: split it back into lines
call assert_equal(['', '1', '2'], split(execute(['echo 1', 'echo 2']), "\n", 1))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'execute_capture.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'execute_capture.vim: all assertions passed'
