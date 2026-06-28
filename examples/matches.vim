" matches.vim — matchadd()/matchaddpos()/matchdelete()/getmatches()/
" setmatches()/clearmatches()/matcharg(), the match-highlight list ported from
" Neovim's window.c. Standalone it is pure in-memory bookkeeping.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- a fresh match list is empty
call assert_equal([], getmatches())

" --- matchadd() with an explicit id returns that id and records the match
call assert_equal(42, matchadd('Error', 'foo', 20, 42))
call assert_equal([{'group': 'Error', 'pattern': 'foo', 'priority': 20, 'id': 42}], getmatches())

" --- the default priority is 10; an auto id (-1, the default) is positive
let auto = matchadd('Search', 'bar')
call assert_true(auto > 0)
call assert_notequal(42, auto)
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
