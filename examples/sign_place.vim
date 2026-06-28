" sign_place.vim — sign_place()/sign_getplaced()/sign_unplace()/
" sign_placelist()/sign_unplacelist()/sign_jump(), the placed-sign list ported
" from Neovim's sign.c. Standalone, buffers are just numbers and the placement
" list is in-memory bookkeeping.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- nothing placed initially
call assert_equal([], sign_getplaced())

" --- sign_place() with id 0 auto-assigns; an explicit id is kept
call assert_equal(1, sign_place(0, 'g1', 'err', 1, {'lnum': 10, 'priority': 20}))
call assert_equal(5, sign_place(5, 'g1', 'warn', 1, {'lnum': 12}))
call assert_equal(2, sign_place(0, 'g2', 'err', 2, {'lnum': 3}))

" --- sign_getplaced() groups placed signs by buffer
let all = sign_getplaced()
call assert_equal(2, len(all))
call assert_equal(1, all[0].bufnr)
call assert_equal(2, len(all[0].signs))
call assert_equal({'id': 1, 'group': 'g1', 'name': 'err', 'bufnr': 1, 'lnum': 10, 'priority': 20}, all[0].signs[0])
" the default priority is 10 when omitted (sign with id 5)
call assert_equal(10, all[0].signs[1].priority)

" --- filter by buffer, then additionally by group
call assert_equal(1, len(sign_getplaced(1)))
call assert_equal(2, len(sign_getplaced(1)[0].signs))
call assert_equal(2, len(sign_getplaced(1, {'group': 'g1'})[0].signs))

" --- sign_unplace() removes a whole group
call assert_equal(0, sign_unplace('g2'))
call assert_equal(1, len(sign_getplaced()))

" --- sign_placelist()/sign_unplacelist() operate on a list of placements
call assert_equal([3], sign_placelist([{'group': 'g3', 'name': 'x', 'buffer': 1, 'lnum': 7}]))
call assert_equal(3, len(sign_getplaced(1)[0].signs))
call assert_equal([0], sign_unplacelist([{'group': 'g3', 'id': 3}]))
call assert_equal(2, len(sign_getplaced(1)[0].signs))

" --- sign_jump() has no editor cursor to move standalone
call assert_equal(-1, sign_jump(1, 'g1', 1))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'sign_place.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'sign_place.vim: all assertions passed'
