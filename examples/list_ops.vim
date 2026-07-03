" list_ops.vim — the mutating / searching List builtins (list.c: insert/add,
" remove single+range+negative, count, index, extend, uniq). insert/add/remove
" mutate the list in place and return either the list or the removed element;
" count/index search by value (with an optional start and, for count, a
" case-insensitivity flag). Self-test: asserts into v:errors, throws if failed.

" --- insert(): prepend by default, or before a given (possibly negative) index
call assert_equal([0, 1, 2, 3], insert([1, 2, 3], 0))
call assert_equal([1, 9, 2, 3], insert([1, 2, 3], 9, 1))
call assert_equal([1, 2, 9, 3], insert([1, 2, 3], 9, -1))
call assert_equal([1], insert([], 1))

" --- add(): append one element, returning the (now longer) list
call assert_equal([1, 2, 3], add([1, 2], 3))
call assert_equal(['x'], add([], 'x'))

" --- remove(): by index returns the element; by range returns the sub-list
call assert_equal(2, remove([1, 2, 3, 4], 1))
call assert_equal([2, 3], remove([1, 2, 3, 4], 1, 2))
call assert_equal(4, remove([1, 2, 3, 4], -1))
call assert_equal([2, 3, 4], remove([1, 2, 3, 4, 5], 1, 3))
" mutation is visible on the bound list
let m = [10, 20, 30]
call assert_equal(20, remove(m, 1))
call assert_equal([10, 30], m)

" --- count(): occurrences of a value; works on lists (incl. nested) and strings
call assert_equal(3, count([1, 2, 2, 3, 2], 2))
call assert_equal(2, count([[1], [1], [2]], [1]))
call assert_equal(3, count('banana', 'a'))
call assert_equal(2, count('aAaA', 'a'))
call assert_equal(4, count('aAaA', 'a', 1))

" --- index(): first position of a value, -1 if absent; optional start/icase
call assert_equal(1, index([10, 20, 30], 20))
call assert_equal(3, index([10, 20, 30, 20], 20, 2))
call assert_equal(0, index(['a', 'B', 'a'], 'A', 0, 1))
call assert_equal(-1, index([1, 2, 3], 5))
call assert_equal(-1, index([], 1))

" --- extend(): splice src into dest in place, optional insertion index
call assert_equal([1, 2, 3, 4], extend([1, 2], [3, 4]))
call assert_equal([3, 4, 1, 2], extend([1, 2], [3, 4], 0))
call assert_equal([1, 9, 2], extend([1, 2], [9], 1))

" --- uniq(): drop ADJACENT duplicates only (non-adjacent repeats survive)
call assert_equal([1, 2, 3, 1], uniq([1, 1, 2, 2, 2, 3, 1]))
call assert_equal(['a', 'b', 'a'], uniq(['a', 'a', 'b', 'a']))
call assert_equal([1], uniq([1, 1, 1]))
call assert_equal([], uniq([]))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'list_ops.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'list_ops.vim: all assertions passed'
