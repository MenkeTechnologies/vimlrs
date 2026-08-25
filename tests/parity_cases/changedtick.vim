" b:changedtick — the buffer's own edit counter.
"
" c: it is not an entry of b_vars but the buffer's changedtick_di, a
" DI_FLAGS_RO|DI_FLAGS_FIX dictitem read through buf_get_changedtick()
" (Src/buffer.h:84) and bumped by buf_inc_changedtick() from changed_common()
" via changed(buf) (vendor/change.c:143). Every edit path converges there:
" changed_bytes() (c:425) once per REPLACED line, appended_lines_mark() (c:484)
" once per appended BLOCK, deleted_lines_mark() (c:509) once per deleted block.
"
" That asymmetry is the whole point of the probe list below — setline() over two
" existing lines moves the tick by 2 while append() of two lines moves it by 1.

echo 'exists       =' exists('b:changedtick') 'type=' type(b:changedtick)
echo 'start        =' b:changedtick

call setline(1, 'a')
echo 'setline      =' b:changedtick
" ... even when the text is identical: ml_replace runs either way.
call setline(1, 'a')
echo 'setline same =' b:changedtick
call setline(1, 'b')
echo 'setline diff =' b:changedtick

call append(1, 'c')
echo 'append 1     =' b:changedtick
call append(1, ['d', 'e'])
echo 'append 2     =' b:changedtick

call setline(1, ['x', 'y'])
echo 'setline list =' b:changedtick

call deletebufline('', 1)
echo 'delete one   =' b:changedtick
call deletebufline('', 1, 2)
echo 'delete range =' b:changedtick

call setbufline('', 1, 'z')
echo 'setbufline   =' b:changedtick
call appendbufline('', 0, 'w')
echo 'appendbufline=' b:changedtick

" Anything that writes nothing moves nothing.
let g:x = 1
echo 'no edit      =' b:changedtick
call setline(99, 'far')
echo 'setline oob  =' b:changedtick
call setline(0, 'zero')
echo 'setline 0    =' b:changedtick
call append(0, [])
echo 'append empty =' b:changedtick
call setline(1, [])
echo 'setline empty=' b:changedtick
call deletebufline('', 99)
echo 'delete oob   =' b:changedtick

" A setline() that runs off the end replaces what it can and appends the rest:
" one bump per replaced line, plus one for the whole appended block.
echo 'lines now    =' line('$')
call setline(line('$'), ['A', 'B'])
echo 'setline span =' b:changedtick '$=' line('$')

" It is read-only.
let v:errmsg = ''
silent! let b:changedtick = 5
echo 'assign errmsg=' v:errmsg
echo 'still        =' b:changedtick
