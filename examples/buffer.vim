" buffer.vim — text manipulation on the in-memory current buffer: getline/
" setline/append/deletebufline plus line()/col()/cursor()/indent()/line2byte()
" (ported from Neovim's buffer.c / memline.c). vimlrs runs standalone but keeps
" one virtual buffer that scripts populate.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- setline() fills the buffer; line('$') is the line count
call setline(1, ['alpha', 'beta', 'gamma'])
call assert_equal(3, line('$'))
call assert_equal('beta', getline(2))
call assert_equal(['alpha', 'beta'], getline(1, 2))

" --- append() inserts after a line (0 = before the first)
call append(1, 'inserted')
call assert_equal(['alpha', 'inserted', 'beta', 'gamma'], getline(1, '$'))
call append(0, 'top')
call assert_equal('top', getline(1))

" --- setline() replaces; a List replaces several lines at once
call setline(2, 'ALPHA')
call assert_equal('ALPHA', getline(2))
call setline(1, ['x', 'y'])
call assert_equal(['x', 'y'], getline(1, 2))

" --- deletebufline() removes a range; the buffer keeps >= 1 line.
"     (setline() only replaces the lines given, so clear the buffer first.)
call deletebufline('', 1, '$')
call setline(1, ['one', 'two', 'three', 'four'])
call deletebufline('', 2, 3)
call assert_equal(['one', 'four'], getline(1, '$'))

" --- cursor()/line('.')/col('.')/col('$') track an in-buffer position
call setline(1, ['hello world', 'second line'])
call cursor(1, 7)
call assert_equal(1, line('.'))
call assert_equal(7, col('.'))
" 'hello world' is 11 bytes, so the end-of-line column is 12
call assert_equal(12, col('$'))
call assert_equal([0, 1, 7, 0, 7], getcurpos())

" --- setpos('.', …) / getpos('.') round-trip the cursor
call setpos('.', [0, 2, 3, 0])
call assert_equal([0, 2, 3, 0], getpos('.'))

" --- indent() measures the leading whitespace (Tabs expand to 'tabstop')
call setline(1, ['    four spaces', "\ttab then text", 'none'])
call assert_equal(4, indent(1))
call assert_equal(8, indent(2))
call assert_equal(0, indent(3))

" --- line2byte()/byte2line() convert between line and byte offsets
call setline(1, ['abc', 'de', 'fghi'])
call assert_equal(1, line2byte(1))
" 'abc' (3 bytes) + newline (1) + 1 = 5
call assert_equal(5, line2byte(2))
call assert_equal(2, byte2line(5))

" --- nextnonblank()/prevnonblank() skip blank (whitespace-only) lines
call setline(1, ['a', '', '   ', 'b'])
call assert_equal(4, nextnonblank(2))
call assert_equal(1, prevnonblank(3))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'buffer.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'buffer.vim: all assertions passed'
