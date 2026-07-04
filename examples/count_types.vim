" count_types.vim — count() across the three container kinds (eval.c). On a
" String it counts NON-OVERLAPPING occurrences of a substring; on a List it
" counts elements equal to the item; on a Dict it counts VALUES equal to the
" item. The third arg is `ic` (ignore-case); a List also takes a fourth `start`
" index. Non-overlap means 'aaa' contains 'aa' once, not twice. These are the
" String/Dict paths not exercised by list_edit.vim's List-only cases.
" Self-test: asserts into v:errors, throws if any failed.

" --- count() on a String: non-overlapping substring occurrences
call assert_equal(3, count('aXbXcX', 'X'))
call assert_equal(2, count('hello', 'l'))
call assert_equal(3, count('abcabcabc', 'bc'))
call assert_equal(2, count('Hello World Hello', 'Hello'))
call assert_equal(0, count('abcabc', 'x'))
call assert_equal(0, count('', 'x'))

" --- count() on a String: non-overlap ('aaa' has 'aa' once, 'ss' twice in miss.)
call assert_equal(1, count('aaa', 'aa'))
call assert_equal(2, count('mississippi', 'ss'))

" --- count() on a String: ic=1 folds case, ic=0 is exact
call assert_equal(2, count('AaAa', 'a', 0))
call assert_equal(4, count('AaAa', 'a', 1))

" --- count() on a String: multibyte substrings count by character content
call assert_equal(3, count('日本日本日', '日'))

" --- count() on a Dict counts matching VALUES, not keys
call assert_equal(2, count({'a': 1, 'b': 2, 'c': 1}, 1))
call assert_equal(1, count({'a': 'x', 'b': 'y'}, 'x'))
call assert_equal(0, count({'a': 1, 'b': 2}, 9))
call assert_equal(0, count({}, 1))

" --- count() on a Dict with ic=1 folds case of String values
call assert_equal(2, count({'a': 'X', 'b': 'x', 'c': 'y'}, 'x', 1))

" --- count() on a List for contrast: ic folds case of String elements
call assert_equal(3, count(['a', 'A', 'a'], 'a', 1))
call assert_equal(2, count(['a', 'A', 'a'], 'a', 0))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'count_types.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'count_types.vim: all assertions passed'
