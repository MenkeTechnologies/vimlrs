" The cursor moves with the text around it.
"
" c: set_buffer_lines (vendor/eval/buffer.c:221-231) runs
" appended_lines_mark(append_lnum, added) and then, for the window showing the
" buffer, `if (wp->w_cursor.lnum > append_lnum) wp->w_cursor.lnum += added;` —
" text inserted ABOVE the cursor pushes it down, text inserted below leaves it
" alone. f_deletebufline (c:550-562) is the mirror: a cursor BELOW the deleted
" block moves up by the count, one INSIDE it lands on `first`, and either way it
" is clamped to the shortened buffer.
"
" NOT probed: line('.') before anything has been written. vim answers 0 there
" and this port answers 1; that is a silent-Ex startup state, not an edit rule.

call setline(1, ['a', 'b', 'c', 'd', 'e'])
echo 'after setline   line=' line('.') '$=' line('$')

call cursor(3, 1)
echo 'cursor 3        line=' line('.')

call append(1, 'X')
echo 'append above    line=' line('.') '$=' line('$')
call append(5, 'Y')
echo 'append below    line=' line('.') '$=' line('$')
call append(line('.') - 1, ['P', 'Q'])
echo 'append 2 above  line=' line('.') '$=' line('$')

call deletebufline('', 1)
echo 'delete above    line=' line('.') '$=' line('$')
call deletebufline('', line('$'))
echo 'delete below    line=' line('.') '$=' line('$')

" A delete that spans the cursor drops it on the first deleted line.
call cursor(4, 1)
echo 'cursor 4        line=' line('.')
call deletebufline('', 3, 5)
echo 'delete spanning line=' line('.') '$=' line('$')

" ... and a delete that empties the buffer clamps it to 1.
call setline(1, ['p', 'q', 'r'])
call cursor(3, 1)
call deletebufline('', 1, 99)
echo 'delete all      line=' line('.') '$=' line('$')

" setline() that runs off the end appends, and that counts as an insertion.
call setline(1, ['a', 'b', 'c'])
call cursor(1, 1)
call setline(3, ['C', 'D', 'E'])
echo 'setline past    line=' line('.') '$=' line('$')
call cursor(3, 1)
call setline(1, 'A')
echo 'setline replace line=' line('.') '$=' line('$')

" A rejected edit moves nothing.
call cursor(2, 1)
call deletebufline('', 99)
echo 'delete oob      line=' line('.') '$=' line('$')
call setline(0, 'zero')
echo 'setline 0       line=' line('.') '$=' line('$')
call append(0, [])
echo 'append empty    line=' line('.') '$=' line('$')
