" dict_ops.vim — the Dictionary builtins (dict.c/eval.c: keys/values/items,
" get/has_key, extend/filter/map, remove/copy). keys()/values()/items() return
" lists in the dict's internal hash order, which differs between engines, so the
" order-sensitive cases sort() the result first; assert_equal on a Dictionary
" compares content, not key order, so map/filter/extend/copy need no sorting.
" Self-test: asserts into v:errors, throws if any failed.

" --- keys(): the key list (sorted here — hash order is unspecified)
call assert_equal(['a', 'm', 'z'], sort(keys({'z': 1, 'a': 2, 'm': 3})))
call assert_equal([], keys({}))

" --- values(): the value list (sorted numerically to normalise hash order)
call assert_equal([1, 2, 3], sort(values({'z': 1, 'a': 2, 'm': 3}), 'n'))
call assert_equal([], values({}))

" --- items(): [key, value] pairs (sorted by key for a stable comparison)
call assert_equal([['a', 1], ['b', 2]], sort(items({'b': 2, 'a': 1})))
call assert_equal([], items({}))

" --- has_key(): membership test
call assert_equal(1, has_key({'a': 1}, 'a'))
call assert_equal(0, has_key({'a': 1}, 'z'))
call assert_equal(0, has_key({}, 'a'))

" --- get(): value or default (missing key without default yields 0)
call assert_equal(1, get({'a': 1}, 'a', -1))
call assert_equal(-1, get({'a': 1}, 'z', -1))
call assert_equal(0, get({}, 'k'))

" --- extend(): merge src into dict in place; third arg controls collisions
call assert_equal({'a': 1, 'b': 2}, extend({'a': 1}, {'b': 2}))
call assert_equal({'a': 9, 'b': 2}, extend({'a': 1}, {'a': 9, 'b': 2}))
call assert_equal({'a': 1, 'b': 2}, extend({'a': 1}, {'a': 9, 'b': 2}, 'keep'))

" --- filter(): keep entries where the expr is truthy (k, v lambda args)
call assert_equal({'b': 2, 'c': 3}, filter({'a': 1, 'b': 2, 'c': 3}, {k, v -> v > 1}))
call assert_equal({}, filter({'a': 1, 'b': 2}, {k, v -> 0}))

" --- map(): transform every value in place
call assert_equal({'a': 10, 'b': 20}, map({'a': 1, 'b': 2}, {k, v -> v * 10}))
call assert_equal({'a': 'a1', 'b': 'b2'}, map({'a': 1, 'b': 2}, {k, v -> k . v}))

" --- remove(): delete a key, returning the removed value; dict shrinks
let d = {'a': 1, 'b': 2}
call assert_equal(1, remove(d, 'a'))
call assert_equal({'b': 2}, d)

" --- copy(): a shallow, independent top level (nested refs still shared)
let src = {'a': 1}
let cp = copy(src)
let cp['a'] = 99
call assert_equal({'a': 1}, src)
call assert_equal({'a': 99}, cp)

" --- len()/empty() on dicts
call assert_equal(2, len({'a': 1, 'b': 2}))
call assert_equal(1, empty({}))
call assert_equal(0, empty({'a': 1}))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'dict_ops.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'dict_ops.vim: all assertions passed'
