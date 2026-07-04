" list_edit.vim — the in-place list mutators and queries (list.c: add appends,
" insert prepends or inserts at an index, remove deletes one item or an index
" range and RETURNS what it removed, count tallies occurrences, index finds the
" first position). insert/remove/add mutate the list AND return it (remove
" returns the removed element/slice). range() builds the numeric lists these
" operate on. Self-test: asserts into v:errors, throws if any failed.

" --- add(): append one item, returning the (grown) list
let a = [1, 2]
call assert_equal([1, 2, 7], add(a, 7))
call assert_equal([1, 2, 7], a)

" --- insert(): default prepends; optional index inserts before that position
call assert_equal([9, 1, 2, 3], insert([1, 2, 3], 9))
call assert_equal([1, 9, 2, 3], insert([1, 2, 3], 9, 1))
let tail = [1, 2, 3]
call assert_equal([1, 2, 3, 9], insert(tail, 9, len(tail)))

" --- remove(): single index returns the removed element and shrinks the list
let b = [1, 2, 3, 4]
call assert_equal(3, remove(b, 2))
call assert_equal([1, 2, 4], b)

" --- remove(): a negative index counts from the end
let c = [1, 2, 3, 4, 5]
call assert_equal(5, remove(c, -1))
call assert_equal([1, 2, 3, 4], c)

" --- remove(): an index RANGE returns the removed slice as a list
let d = [1, 2, 3, 4, 5]
call assert_equal([2, 3, 4], remove(d, 1, 3))
call assert_equal([1, 5], d)
call assert_equal([2, 3, 4], remove([0, 1, 2, 3, 4], -3, -1))

" --- count(): number of matching elements; optional [ic, start] window
call assert_equal(3, count([1, 2, 2, 3, 2], 2))
call assert_equal(0, count([1, 2, 3], 99))
call assert_equal(1, count([1, 2, 3, 2, 1], 2, 0, 2))

" --- index(): position of first match, -1 if absent, with optional start / ic
call assert_equal(1, index([10, 20, 30], 20))
call assert_equal(-1, index([1, 2, 3], 99))
call assert_equal(2, index(['a', 'B', 'a'], 'a', 1))
call assert_equal(0, index(['A', 'b'], 'a', 0, 1))

" --- range(): {end} | {start,end} | {start,end,stride} (end is inclusive)
call assert_equal([0, 1, 2], range(3))
call assert_equal([2, 3, 4, 5], range(2, 5))
call assert_equal([0, 3, 6, 9], range(0, 10, 3))
call assert_equal([], range(0))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'list_edit.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'list_edit.vim: all assertions passed'
