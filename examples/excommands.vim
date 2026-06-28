" excommands.vim — buffer-editing Ex commands with line ranges (ported from
" Neovim's ex_cmds.c / ex_docmd.c): :[range]s/g/v/d/m/t/j/y/pu. They run on the
" in-memory buffer, so they work fully standalone. A ':'-prefixed (or '%'-
" prefixed) line is an Ex command; an unrecognized one runs as a statement.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- :%s substitutes over the whole buffer (default: first match per line)
call setline(1, ['foo a foo', 'bar', 'foo c'])
:%s/foo/X/
call assert_equal(['X a foo', 'bar', 'X c'], getline(1, '$'))

" --- the g flag substitutes every match on each line
call deletebufline('', 1, '$')
call setline(1, ['foo a foo'])
:%s/foo/X/g
call assert_equal(['X a X'], getline(1, '$'))

" --- :[range]d deletes a line range
call deletebufline('', 1, '$')
call setline(1, ['a', 'b', 'c', 'd', 'e'])
:2,4d
call assert_equal(['a', 'e'], getline(1, '$'))

" --- :g/pat/d deletes every matching line; :v/pat/d keeps only matching lines
call deletebufline('', 1, '$')
call setline(1, ['keep1', 'DROP', 'keep2', 'DROP', 'keep3'])
:g/DROP/d
call assert_equal(['keep1', 'keep2', 'keep3'], getline(1, '$'))

call deletebufline('', 1, '$')
call setline(1, ['yes a', 'no b', 'yes c', 'no d'])
:v/yes/d
call assert_equal(['yes a', 'yes c'], getline(1, '$'))

" --- :[range]m{addr} moves lines; :[range]t{addr} (copy) duplicates them
call deletebufline('', 1, '$')
call setline(1, ['x', 'y', 'z'])
:1m$
call assert_equal(['y', 'z', 'x'], getline(1, '$'))

call deletebufline('', 1, '$')
call setline(1, ['A', 'B', 'C'])
:1t2
call assert_equal(['A', 'B', 'A', 'C'], getline(1, '$'))

" --- :[range]j joins lines into one (leading whitespace of joined lines drops)
call deletebufline('', 1, '$')
call setline(1, ['one', '   two', 'three'])
:1,2j
call assert_equal(['one two', 'three'], getline(1, '$'))

" --- :[range]y {reg} yanks to a register; :pu {reg} puts it after the line
call deletebufline('', 1, '$')
call setline(1, ['p', 'q', 'r'])
:1,2y a
:$pu a
call assert_equal(['p', 'q', 'r', 'p', 'q'], getline(1, '$'))

" --- a relative range works ('.' is the cursor, '+N'/'$' offsets)
call deletebufline('', 1, '$')
call setline(1, ['l1', 'l2', 'l3', 'l4'])
call cursor(2, 1)
:.,+1d
call assert_equal(['l1', 'l4'], getline(1, '$'))

" --- an unrecognized ':' command falls back to running as a statement
let g:fallback = 0
:let g:fallback = 42
call assert_equal(42, g:fallback)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'excommands.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'excommands.vim: all assertions passed'
