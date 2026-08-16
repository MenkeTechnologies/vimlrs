" setreg() blockwise width — get_yank_type()'s two halves.
"
" `block_len` starts at -1, which means AUTO: str_to_reg sizes the block from the
" longest line it stored. Only a digit run after `b`/CTRL-V replaces it, and that
" run is part of the SAME type token — `'b3'` is one option, not `b` followed by
" a stray `3`. Passing 0 for the absent-digits case instead of -1 pinned every
" auto-sized block register to `^V1`.

" Explicit width through the {options} string.
call setreg('a', 'z', 'b3')
echo getregtype('a')
call setreg('b', 'z', "\<C-V>5")
echo getregtype('b')

" Auto width from a single string.
call setreg('c', 'abcd', 'b')
echo getregtype('c')

" Auto width from a List — the LONGEST line wins.
call setreg('d', ['ab', 'cde'], 'b')
echo getregtype('d')

" Same two cases through the Dict form's `regtype` key.
call setreg('e', {'regcontents': ['ab', 'cde'], 'regtype': 'b'})
echo getregtype('e')
call setreg('f', {'regcontents': ['ab', 'cde'], 'regtype': 'b7'})
echo getregtype('f')
call setreg('g', {'regcontents': ['ab', 'cde'], 'regtype': "\<C-V>"})
echo getregtype('g')

" A width digit must not be mistaken for another option letter: 'ab2' is append
" plus a 2-wide block, and the append still happens.
call setreg('h', 'xy', 'b')
call setreg('h', 'z', 'ab2')
echo getregtype('h')
echo string(getreg('h', 1, 1))

" The width is reported by getreginfo() too, in the same ^VN form.
echo string(getreginfo('f'))

" Non-block types ignore the width entirely.
call setreg('i', 'z', 'v')
echo getregtype('i')
call setreg('j', 'z', 'V')
echo getregtype('j')

" An auto-sized empty write leaves a 1-wide block (maxlen 0 is still one column).
call setreg('k', '', 'b')
echo getregtype('k')
