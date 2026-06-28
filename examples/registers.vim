" registers.vim — @register access in expressions (let @a = …, echo @a) and the
" buffer-introspection builtins wordcount()/virtcol()/getregion(). Registers are
" shared with :yank/:put, so the two views stay consistent. Runs standalone.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- @r reads a register; let @r = … writes it
let @a = 'hello'
let @b = 'world'
call assert_equal('hello', @a)
call assert_equal('world', @b)
let @a = @a . ' ' . @b
call assert_equal('hello world', @a)

" --- the unnamed register @" works too
let @" = 'unnamed'
call assert_equal('unnamed', @")

" --- @r and the :yank/:put commands share the same registers
call setline(1, ['one', 'two', 'three'])
:2y c
call assert_equal("two\n", @c)
let @d = "X\n"
:$pu d
call assert_equal(['one', 'two', 'three', 'X'], getline(1, '$'))

" --- wordcount() counts bytes/chars/words of the buffer, and up to the cursor
call deletebufline('', 1, '$')
call setline(1, ['hello world', 'foo bar baz'])
call cursor(2, 5)
let wc = wordcount()
call assert_equal(5, wc.words)
call assert_equal(24, wc.bytes)
call assert_equal(3, wc.cursor_words)

" --- virtcol() expands Tabs to 'tabstop' (default 8)
call deletebufline('', 1, '$')
call setline(1, ["a\tb"])
call cursor(1, 2)
call assert_equal(8, virtcol('.'))
call cursor(1, 3)
call assert_equal(9, virtcol('.'))
call assert_equal(10, virtcol('$'))

" --- getregion() returns the text between two positions (charwise / linewise)
call deletebufline('', 1, '$')
call setline(1, ['abcdef', 'ghijkl', 'mnopqr'])
call assert_equal(['cdef', 'ghijkl', 'mn'], getregion([0, 1, 3, 0], [0, 3, 2, 0]))
call assert_equal(['abcdef', 'ghijkl'], getregion([0, 1, 1, 0], [0, 2, 1, 0], {'type': 'V'}))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'registers.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'registers.vim: all assertions passed'
