" repeat.vim — repeat() over String, List, and Blob (funcs.c f_repeat).
" repeat({expr}, {count}) repeats: a String count times, a List's items count
" times (a new List), or a Blob's bytes count times (a new Blob). A count <= 0
" yields the empty value. Self-test into v:errors.

" --- String
call assert_equal('ababab', repeat('ab', 3))
call assert_equal('', repeat('ab', 0))
call assert_equal('', repeat('ab', -1))
call assert_equal('', repeat('', 5))

" --- List (a new list of the items repeated)
call assert_equal([1, 2, 1, 2, 1, 2], repeat([1, 2], 3))
call assert_equal([], repeat([1, 2], 0))
call assert_equal([0, 0, 0], repeat([0], 3))

" --- Blob (the bytes repeated)
call assert_equal(0z010201020102, repeat(0z0102, 3))
call assert_equal(0z, repeat(0z0102, 0))
call assert_equal(0z, repeat(0z, 5))

" --- repeat() of a Number stringifies it first (as Vim does)
call assert_equal('555', repeat(5, 3))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'repeat.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'repeat.vim: all assertions passed'
