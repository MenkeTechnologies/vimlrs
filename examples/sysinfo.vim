" sysinfo.vim — standalone-environment builtins: hostname(), iconv(),
" setcellwidths()/getcellwidths() (Neovim mbyte.c), and the argument-list /
" fold introspection that is empty when run outside an editor.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- hostname() returns the (non-empty) system host name
call assert_true(hostname() != '')
call assert_equal(type(''), type(hostname()))

" --- iconv() is identity for the same (or UTF-8) encoding
call assert_equal('hello', iconv('hello', 'utf-8', 'utf-8'))
call assert_equal('héllo', iconv('héllo', 'latin1', 'latin1'))
call assert_equal('hello', iconv('hello', 'latin1', 'utf-8'))

" --- setcellwidths() overrides display width; getcellwidths() returns the table
call assert_equal(1, strwidth('A'))
call setcellwidths([[0x41, 0x41, 2]])
call assert_equal(2, strwidth('A'))
call assert_equal([[65, 65, 2]], getcellwidths())
" clear the override so it does not leak to other width checks
call setcellwidths([])
call assert_equal(1, strwidth('A'))
call assert_equal([], getcellwidths())

" --- a cell-width override can also widen a range of codepoints
call setcellwidths([[0x2600, 0x26ff, 2]])
call assert_equal(2, strwidth('☀'))
call setcellwidths([])

" --- no editor argument list standalone: argc()/argv()/argidx()/arglistid()
call assert_equal(0, argc())
call assert_equal([], argv())
call assert_equal('', argv(0))
call assert_equal(0, argidx())
call assert_equal(0, arglistid())

" --- no folds standalone: every line is at fold level 0
call assert_equal(0, foldlevel(1))
call assert_equal(0, foldlevel(999))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'sysinfo.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'sysinfo.vim: all assertions passed'
