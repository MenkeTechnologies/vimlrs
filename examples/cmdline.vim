" cmdline.vim — setcmdline()/getcmdline()/setcmdpos()/getcmdpos()/getcmdtype(),
" the command-line buffer ported from Neovim's ex_getln.c. Standalone it is a
" settable in-memory command line.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- no command line yet: empty contents, cursor at 0, no type
call assert_equal('', getcmdline())
call assert_equal(0, getcmdpos())
call assert_equal('', getcmdtype())

" --- setcmdline() sets the contents and puts the cursor past the end (len+1)
call assert_equal(0, setcmdline('echo 42'))
call assert_equal('echo 42', getcmdline())
call assert_equal(8, getcmdpos())

" --- setcmdline() with an explicit byte position
call assert_equal(0, setcmdline('let x = 1', 5))
call assert_equal('let x = 1', getcmdline())
call assert_equal(5, getcmdpos())

" --- setcmdpos() moves just the cursor
call assert_equal(0, setcmdpos(2))
call assert_equal(2, getcmdpos())
call assert_equal('let x = 1', getcmdline())

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'cmdline.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'cmdline.vim: all assertions passed'
