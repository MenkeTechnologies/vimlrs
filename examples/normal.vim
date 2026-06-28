" normal.vim — the :normal command runs normal-mode keys on the buffer (ported
" from Neovim's normal.c / ops.c, a bounded subset: motions, delete/yank/put,
" and simple edits). No insert mode, so i/a/o are not supported.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- x deletes the char under the cursor; dd deletes the line
call setline(1, ['hello', 'world', 'third'])
call cursor(1, 1)
:normal x
call assert_equal('ello', getline(1))
:normal dd
call assert_equal(['world', 'third'], getline(1, '$'))

" --- counts repeat a command (3x deletes three chars)
call deletebufline('', 1, '$')
call setline(1, ['abcdefg'])
call cursor(1, 1)
:normal 3x
call assert_equal('defg', getline(1))

" --- motions move the cursor: $ to end, 0 to start, | to a column, G to a line
call deletebufline('', 1, '$')
call setline(1, ['alpha beta', 'gamma', 'delta'])
call cursor(1, 1)
:normal $
call assert_equal(10, col('.'))
:normal 0
call assert_equal(1, col('.'))
:normal G
call assert_equal(3, line('.'))
:normal gg
call assert_equal(1, line('.'))

" --- word motions: w to next word, dw deletes a word
call cursor(1, 1)
:normal w
call assert_equal(7, col('.'))
call cursor(1, 1)
:normal dw
call assert_equal('beta', getline(1))

" --- yy/p copy and paste a line (linewise)
call deletebufline('', 1, '$')
call setline(1, ['one', 'two', 'three'])
call cursor(1, 1)
:normal yy
call cursor(3, 1)
:normal p
call assert_equal(['one', 'two', 'three', 'one'], getline(1, '$'))

" --- r replaces a char, ~ toggles case, D deletes to end of line
call deletebufline('', 1, '$')
call setline(1, ['Hello World'])
call cursor(1, 1)
:normal rJ
call assert_equal('Jello World', getline(1))
:normal ~
call assert_equal('jello World', getline(1))
call cursor(1, 6)
:normal D
call assert_equal('jello', getline(1))

" --- a :[range]normal runs the keys on every line in the range
call deletebufline('', 1, '$')
call setline(1, ['Xabc', 'Xdef', 'Xghi'])
:%normal x
call assert_equal(['abc', 'def', 'ghi'], getline(1, '$'))

" --- J joins lines
call deletebufline('', 1, '$')
call setline(1, ['foo', 'bar', 'baz'])
call cursor(1, 1)
:normal J
call assert_equal('foo bar', getline(1))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'normal.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'normal.vim: all assertions passed'
