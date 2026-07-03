" list_transform.vim — the non-mutating / structural list & dict builtins
" (funcs.c: flatten/flattennew, extendnew, mapnew, deepcopy, foreach). The
" *new-suffixed forms and deepcopy must leave their input untouched and return
" an independent copy; the mutation-independence of that copy is asserted, not
" just the shape. Self-test: asserts into v:errors, throws if any failed.

" --- flatten(): recursively splice nested lists (mutates + returns the list)
call assert_equal([1, 2, 3, 4], flatten([1, [2, [3, [4]]]]))
call assert_equal([1, 2, [3, [4]]], flatten([1, [2, [3, [4]]]], 1))
call assert_equal([], flatten([]))
call assert_equal([], flatten([[[]]]))

" --- flattennew(): same, but the source list is left unmodified
let nested = [1, [2, [3]]]
call assert_equal([1, 2, 3], flattennew(nested))
call assert_equal([1, 2, [3]], flattennew(nested, 1))
call assert_equal([1, [2, [3]]], nested)

" --- extendnew(): concatenate into a fresh list/dict, source unchanged
let base = [1, 2]
call assert_equal([1, 2, 3, 4], extendnew(base, [3, 4]))
call assert_equal([1, 2], base)
call assert_equal([1, 9, 2, 3], extendnew([1, 2, 3], [9], 1))
call assert_equal([2, 1], extendnew([1], [2], 0))
call assert_equal({'a': 1, 'b': 2}, extendnew({'a': 1}, {'b': 2}))

" --- mapnew(): map into a fresh list/dict, source unchanged
let src = [1, 2, 3]
call assert_equal([1, 4, 9], mapnew(src, {i, v -> v * v}))
call assert_equal([1, 3, 5], mapnew(src, {i, v -> v + i}))
call assert_equal([1, 2, 3], src)
call assert_equal({'a': 11, 'b': 12}, mapnew({'a': 1, 'b': 2}, {k, v -> v + 10}))

" --- deepcopy(): nested copy is fully independent of the original
let orig = [[1, 2], {'c': 3}]
let dup = deepcopy(orig)
let dup[0][0] = 99
let dup[1]['c'] = 88
call assert_equal([[1, 2], {'c': 3}], orig)
call assert_equal([[99, 2], {'c': 88}], dup)

" --- foreach(): iterate for side effects, returns the (unchanged) input list
call assert_equal([10, 20], foreach([10, 20], {i, v -> 0}))
let g:seen = []
call foreach(['a', 'b', 'c'], {i, v -> add(g:seen, v)})
call assert_equal(['a', 'b', 'c'], g:seen)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'list_transform.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'list_transform.vim: all assertions passed'
