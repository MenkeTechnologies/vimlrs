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
call assert_equal(1, strwidth('☀'))
call assert_equal([], getcellwidths())
call setcellwidths([[0x2600, 0x26ff, 2]])
call assert_equal(2, strwidth('☀'))
call assert_equal([[9728, 9983, 2]], getcellwidths())
" clear the override so it does not leak to other width checks
call setcellwidths([])
call assert_equal(1, strwidth('☀'))
call assert_equal([], getcellwidths())

" --- ASCII cannot be overridden at all: an entry below 0x80 is rejected with
"     E1114 and the table is left exactly as it was.
let s:err = ''
call assert_equal(1, strwidth('A'))
try
  call setcellwidths([[0x41, 0x41, 2]])
catch
  let s:err = v:exception
endtry
call assert_equal('Vim(call):E1114: Only values of 0x80 and higher supported', s:err)
call assert_equal(1, strwidth('A'))
call assert_equal([], getcellwidths())

" --- the table is reported sorted on the first codepoint, NOT in input order
call setcellwidths([[0x2700, 0x2700, 2], [0x2600, 0x2600, 2]])
call assert_equal([[9728, 9728, 2], [9984, 9984, 2]], getcellwidths())

" --- a rejected update leaves the PREVIOUS table installed (E1113 here: the
"     two ranges overlap at 0x2640 once sorted on their first codepoint)
let s:err = ''
try
  call setcellwidths([[0x2600, 0x2650, 2], [0x2640, 0x2700, 1]])
catch
  let s:err = v:exception
endtry
call assert_equal('Vim(call):E1113: Overlapping ranges for 0x2640', s:err)
call assert_equal([[9728, 9728, 2], [9984, 9984, 2]], getcellwidths())
call setcellwidths([])
call assert_equal([], getcellwidths())

" --- the argument list is the script file(s) on the command line; this script
"     is the sole argument (see arglist.vim for the full behaviour)
call assert_equal(1, argc())
call assert_match('sysinfo\.vim$', argv(0))
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
