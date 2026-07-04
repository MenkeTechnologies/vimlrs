" dict_deepcopy.vim — Dictionary copy semantics and extend() collision policy
" (eval.c/dict.c). copy() is SHALLOW: the top-level Dict is new but nested
" containers stay shared, so mutating a nested list through the copy is visible
" through the original. deepcopy() recurses, giving fully independent nested
" containers. extend(d, d2, action) resolves key collisions by action: 'force'
" overwrites (the default), 'keep' preserves d's value, 'error' throws E737.
" extendnew() is extend() that returns a fresh Dict and leaves both args intact.
" Self-test: asserts into v:errors, throws if any failed.

" --- deepcopy(): nested list is INDEPENDENT of the source
let src = {'nums': [1, 2], 'name': 'x'}
let dc = deepcopy(src)
call add(dc.nums, 99)
call assert_equal([1, 2], src.nums)
call assert_equal([1, 2, 99], dc.nums)

" --- deepcopy(): nested dict is independent too
let s3 = {'inner': {'k': 1}}
let d3 = deepcopy(s3)
let d3.inner.k = 5
call assert_equal(1, s3.inner.k)
call assert_equal(5, d3.inner.k)

" --- copy(): SHALLOW — the nested list is SHARED with the source
let s2 = {'nums': [1, 2]}
let cp = copy(s2)
call add(cp.nums, 99)
call assert_equal([1, 2, 99], s2.nums)
call assert_equal([1, 2, 99], cp.nums)

" --- copy(): but the TOP level is independent (a new key doesn't leak back)
let base = {'a': 1}
let sh = copy(base)
let sh.b = 2
call assert_equal({'a': 1}, base)
call assert_equal({'a': 1, 'b': 2}, sh)

" --- extend(): default action overwrites on collision (same as 'force')
call assert_equal({'a': 9, 'b': 2}, extend({'a': 1}, {'a': 9, 'b': 2}, 'force'))

" --- extend(): 'keep' preserves the destination's existing value
call assert_equal({'a': 1, 'b': 2}, extend({'a': 1}, {'a': 9, 'b': 2}, 'keep'))

" --- extend(): 'error' throws E737 when a key already exists (no collision = ok)
call assert_equal({'a': 1, 'b': 2}, extend({'a': 1}, {'b': 2}, 'error'))
call assert_fails('call extend({"a": 1}, {"a": 2}, "error")', 'E737')

" --- extendnew(): returns a new Dict, leaving BOTH arguments unmodified
let l = {'a': 1}
let r = {'b': 2}
call assert_equal({'a': 1, 'b': 2}, extendnew(l, r))
call assert_equal({'a': 1}, l)
call assert_equal({'b': 2}, r)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'dict_deepcopy.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'dict_deepcopy.vim: all assertions passed'
