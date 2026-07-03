" reduce_fold.vim — left fold over a list and positional list extend
" (list.c: reduce/extend). reduce() threads an accumulator left-to-right through
" a two-arg funcref; with no seed the first element seeds it, with a seed the
" whole list is folded onto it (empty list returns the seed unchanged). extend()
" with a numeric index splices the second list IN at that position (0 = front,
" len = append). Self-test: asserts into v:errors, throws if any failed.
"
" NOTE: lambda bodies use a spaced '.' for string concatenation ('a . b'); the
" no-space form 'a.b' is a separate parse path — kept out of this file on purpose.

" --- reduce(): numeric folds, seeded and unseeded
call assert_equal(10, reduce([1, 2, 3, 4], {a, b -> a + b}))
call assert_equal(20, reduce([1, 2, 3, 4], {a, b -> a + b}, 10))
call assert_equal(6, reduce([1, 2, 3], {a, b -> a * b}, 1))
call assert_equal(0, reduce([], {a, b -> a + b}, 0))
call assert_equal(5, reduce([5], {a, b -> a + b}))

" --- reduce(): max/min via a ternary in the lambda body
call assert_equal(4, reduce([1, 2, 3, 4], {a, b -> a > b ? a : b}))
call assert_equal(5, reduce([3, 1, 4, 1, 5], {a, b -> a > b ? a : b}, 0))
call assert_equal(1, reduce([3, 1, 4, 1, 5], {a, b -> a < b ? a : b}, 99))

" --- reduce(): string accumulation with a seed prefix
call assert_equal('abc', reduce(['a', 'b', 'c'], {a, b -> a . b}, ''))
call assert_equal('>abc', reduce(['a', 'b', 'c'], {a, b -> a . b}, '>'))
call assert_equal('Zabc', reduce(['a', 'b', 'c'], {acc, x -> acc . x}, 'Z'))

" --- reduce(): building a reversed list as the accumulator
call assert_equal([3, 2, 1], reduce([1, 2, 3], {a, b -> [b] + a}, []))

" --- extend(): positional splice — front, middle, and tail (== append)
call assert_equal([1, 2, 3, 4], extend([1, 2], [3, 4]))
call assert_equal([9, 1, 2, 3], extend([1, 2, 3], [9], 0))
call assert_equal([1, 9, 2, 3], extend([1, 2, 3], [9], 1))
call assert_equal([1, 2, 8, 9, 3], extend([1, 2, 3], [8, 9], 2))
call assert_equal([1, 2, 3, 8, 9], extend([1, 2, 3], [8, 9], 3))
call assert_equal([1, 2], extend([1, 2], []))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'reduce_fold.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'reduce_fold.vim: all assertions passed'
