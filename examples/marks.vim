" marks.vim — buffer marks (ported from Neovim's mark.c): :mark/:k set a mark,
" 'm resolves in positions and line ranges, getpos()/setpos()/line()/col() read
" and write them, getmarklist() lists them. Marks are buffer data, so this runs
" standalone.
" Self-test: asserts into v:errors, throws at the end if anything failed.

call setline(1, ['one', 'two', 'three', 'four', 'five'])

" --- :mark sets a mark at the cursor (its line); getpos('m) reads it
call cursor(2, 1)
:mark a
call cursor(4, 1)
:k b
call assert_equal([0, 2, 1, 0], getpos("'a"))
call assert_equal([0, 4, 1, 0], getpos("'b"))

" --- line('m) / col('m) return the mark's position; an unset mark is 0
call assert_equal(2, line("'a"))
call assert_equal(1, col("'a"))
call assert_equal(0, line("'z"))

" --- setpos('m, …) creates a mark programmatically
call setpos("'c", [0, 5, 3, 0])
call assert_equal([0, 5, 3, 0], getpos("'c"))
call assert_equal(5, line("'c"))
call assert_equal(3, col("'c"))

" --- getmarklist() returns every mark as {mark, pos}
let ml = getmarklist()
call assert_equal(3, len(ml))
call assert_equal("'a", ml[0].mark)
call assert_equal([0, 2, 1, 0], ml[0].pos)

" --- a 'a,'b range addresses the marked lines (here delete lines 2..4)
:'a,'bd
call assert_equal(['one', 'five'], getline(1, '$'))

" --- :delmarks removes named marks; :delmarks! removes them all
:delmarks c
call assert_equal([0, 0, 0, 0], getpos("'c"))
:delmarks!
call assert_equal([], getmarklist())

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'marks.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'marks.vim: all assertions passed'
