" sort_variants.vim — every sort() ordering mode (list.c: default byte order,
" 'n' numeric, 'N' numeric-string, 'i' case-insensitive, 'f' float, a custom
" {a,b->…} comparator, and the legacy numeric flag 1), plus reverse() and the
" sort()+uniq() idiom that dedupes a whole list. sort() is STABLE and mutates
" in place. Self-test: asserts into v:errors, throws if any failed.

" --- default: string/byte comparison (numbers compared as their text form)
call assert_equal([1, 2, 3], sort([3, 1, 2]))
call assert_equal([10, 100, 2, 9], sort([10, 9, 100, 2]))
call assert_equal(['B', 'C', 'a', 'b'], sort(['B', 'a', 'C', 'b']))

" --- 'n': integer numeric order
call assert_equal([2, 9, 10, 100], sort([10, 9, 100, 2], 'n'))
call assert_equal([-5, 0, 3], sort([3, -5, 0], 'n'))

" --- 'N': numeric order over strings that hold numbers
call assert_equal(['2', '9', '10'], sort(['10', '9', '2'], 'N'))

" --- 'i' / legacy 1: case-insensitive string order
call assert_equal(['a', 'B', 'b', 'C'], sort(['B', 'a', 'C', 'b'], 'i'))
call assert_equal(['a', 'b', 'c'], sort(['b', 'a', 'c'], 1))

" --- 'f': floating-point order
call assert_equal([0.5, 1.5, 2.5], sort([1.5, 0.5, 2.5], 'f'))

" --- custom comparator: descending via {a,b -> b - a}
call assert_equal([3, 2, 1], sort([3, 1, 2], {a, b -> b - a}))
" comparator by string length (stable: equal-length keep input order)
call assert_equal(['cc', 'aaa', 'bbb'], sort(['aaa', 'bbb', 'cc'], {a, b -> len(a) - len(b)}))

" --- reverse(): flip order (mutates); pairs naturally with sort()
call assert_equal([3, 2, 1], reverse(sort([3, 1, 2])))
call assert_equal([], reverse([]))

" --- sort()+uniq(): sort first so all duplicates become adjacent, then dedupe
call assert_equal([1, 2, 3], uniq(sort([3, 1, 2, 3, 1, 1])))
call assert_equal([1, 2, 9, 100], uniq(sort([2, 9, 1, 100, 2], 'n')))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'sort_variants.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'sort_variants.vim: all assertions passed'
