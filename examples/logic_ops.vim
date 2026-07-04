" logic_ops.vim — the logical operators && / || / ! and the ?: ternary.
" In Vimscript && and || are NORMALISED booleans: they never yield an operand,
" always 0 or 1, and they short-circuit (the right side is skipped when the
" result is already decided). ! coerces its argument to a boolean and negates
" it. Strings coerce to a number first ('abc' -> 0, '0' -> 0), so a non-empty
" non-numeric string is FALSE. Ternaries nest/chain right-associatively.
" Self-test: asserts into v:errors, throws if any failed.

" --- && returns 1 only when both sides are truthy, else 0 (never the operand)
call assert_equal(1, 1 && 1)
call assert_equal(1, 2 && 3)
call assert_equal(0, 0 && 5)
call assert_equal(1, 1 && 2 && 3)
call assert_equal(1, 5 > 3 && 2 < 4)

" --- || returns 1 when either side is truthy, else 0
call assert_equal(1, 5 || 0)
call assert_equal(0, 0 || 0)
call assert_equal(1, 0 || 7)
call assert_equal(1, 0 || '' || 9)

" --- precedence: && binds tighter than ||
call assert_equal(1, 1 && 0 || 1)
call assert_equal(0, (1 || 0) && 0)

" --- short-circuit: the skipped side is never evaluated (no divide-by-zero)
call assert_equal(1, 1 || 1 / 0)
call assert_equal(0, 0 && 1 / 0)

" --- string operands coerce to a number: only a leading-digit string is truthy
call assert_equal(0, 'abc' && 1)
call assert_equal(1, '0' || 3)
call assert_equal(0, '' || '')
call assert_equal(1, '5xyz' && 1)

" --- ! negates the boolean coercion; a non-empty non-numeric string is false
call assert_equal(0, !5)
call assert_equal(1, !0)
call assert_equal(1, !!42)
call assert_equal(0, !(5 > 3))
call assert_equal(1, !'abc')
call assert_equal(1, !'0')
call assert_equal(0, !'7up')

" --- ternary ?: picks a branch; -1 (any non-zero) is truthy
call assert_equal(2, 1 ? 2 : 3)
call assert_equal(10, -1 ? 10 : 20)
call assert_equal('y', 3 > 2 ? 'y' : 'n')
call assert_equal('e', empty([]) ? 'e' : 'f')

" --- nested / chained ternaries (right-associative): first truthy test wins
call assert_equal(5, 0 ? 2 : 0 ? 4 : 5)
call assert_equal(20, 1 ? 0 ? 10 : 20 : 30)
call assert_equal('mid', 5 == 3 ? 'lo' : 5 == 5 ? 'mid' : 'hi')

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'logic_ops.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'logic_ops.vim: all assertions passed'
