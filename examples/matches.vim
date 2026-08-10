" matches.vim — matchadd()/matchaddpos()/matchdelete()/getmatches()/
" setmatches()/clearmatches()/matcharg(), the match-highlight list ported from
" Neovim's window.c. Standalone it is pure in-memory bookkeeping.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- a fresh match list is empty
call assert_equal([], getmatches())

" --- matchadd() with an explicit id returns that id and records the match
call assert_equal(42, matchadd('Error', 'foo', 20, 42))
call assert_equal([{'group': 'Error', 'pattern': 'foo', 'priority': 20, 'id': 42}], getmatches())

" --- the default priority is 10, and an auto id (-1, the default) comes from a
"     RESERVED range starting at 1000, so it can never collide with a
"     hand-picked id. `auto > 0` / `auto != 42` passed for any positive number,
"     including the 1 a naive counter would hand out; these pin the range, the
"     step, and the default priority. Measured against vim 9.2.0900 and nvim
"     0.12.4: both answer 1000 then 1001.
let auto = matchadd('Search', 'bar')
call assert_equal(1000, auto)
" getmatches() is ordered by ascending priority, so the default-priority match
" just added sorts BEFORE the priority-20 one from above (measured: both
" engines answer the same two-element list in that order).
call assert_equal(10, getmatches()[0].priority)
call assert_equal(1000, getmatches()[0].id)
call assert_equal(20, getmatches()[1].priority)
call assert_equal(1001, matchadd('Search', 'baz'))
call assert_equal(3, len(getmatches()))
" the counter does not rewind when a match is removed
call assert_equal(0, matchdelete(1001))
call assert_equal(1002, matchadd('Search', 'qux'))
call assert_equal(0, matchdelete(1002))
call assert_equal(2, len(getmatches()))

" --- matchdelete() removes by id and returns 0; a missing id returns -1 quietly
call assert_equal(0, matchdelete(42))
call assert_equal(1, len(getmatches()))
call assert_equal('Search', getmatches()[0].group)

" --- setmatches() replaces the whole list; clearmatches() empties it
call assert_equal(0, setmatches([{'group': 'Todo', 'pattern': 'X', 'priority': 5, 'id': 7}]))
call assert_equal([{'group': 'Todo', 'pattern': 'X', 'priority': 5, 'id': 7}], getmatches())
call clearmatches()
call assert_equal([], getmatches())

" --- matchaddpos() records line/column positions instead of a pattern
call assert_equal(3, matchaddpos('Visual', [[1, 2, 3], 5], 10, 3))
let m = getmatches()[0]
call assert_equal('Visual', m.group)
call assert_equal(3, m.id)
call assert_equal([1, 2, 3], m.pos1)
call assert_equal(5, m.pos2)
call clearmatches()

" --- matcharg() reports the :match/:2match/:3match commands (none set here)
call assert_equal(['', ''], matcharg(1))
call assert_equal(['', ''], matcharg(3))
call assert_equal([], matcharg(4))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'matches.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'matches.vim: all assertions passed'
