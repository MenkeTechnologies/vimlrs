" index_get.vim — index() and get() over Lists and Blobs (funcs.c).
" index() finds the first matching element, honouring {start} (negative counts
" from the end) and, for the List form, {ic} (ignore case). get() reads an
" element with an optional default; a String (or any non-container) errors with
" E1531 rather than silently returning the default. Self-test into v:errors.

" --- index(): plain, {start}, negative {start}, out-of-range {start}
call assert_equal(1, index([5, 6, 7], 6))
call assert_equal(3, index([5, 6, 7, 6], 6, 2))
call assert_equal(1, index([5, 6, 7], 6, -2))
call assert_equal(-1, index([1, 2, 3], 2, 5))

" --- index() {ic}: the 4th arg makes the comparison case-insensitive
call assert_equal(1, index(['a', 'B'], 'b', 0, 1))
call assert_equal(-1, index(['a', 'B'], 'b'))
call assert_equal(0, index([['X']], ['x'], 0, 1))

" --- index() over a Blob (by byte), with {start}
call assert_equal(1, index(0z0A0B0C, 11))
call assert_equal(3, index(0z0A0B0C0B, 11, 2))
call assert_equal(-1, index(0z0A0B0C, 11, -1))

" --- get(): default when missing; -1 for a Blob out of range
call assert_equal(2, get([1, 2, 3], 1))
call assert_equal(-1, get([1, 2], 5, -1))
call assert_equal(99, get({'a': 1}, 'b', 99))
call assert_equal(11, get(0z0A0B, 1))
call assert_equal(-1, get(0z0A0B, 9))

" --- get() on a String is an error (E1531), not a silent default
call assert_fails("call get('hello', 1)", 'E1531')

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'index_get.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'index_get.vim: all assertions passed'
