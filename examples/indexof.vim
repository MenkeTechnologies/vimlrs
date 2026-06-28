" indexof.vim — indexof() over List and Blob (funcs.c indexof_list/indexof_blob).
" indexof({obj}, {expr} [, {opts}]) returns the index of the first item for which
" {expr} is true, or -1. {expr} is a string (seeing v:key/v:val) or a funcref
" taking (key, val). {opts.startidx} begins the scan later; the start rules
" differ for List vs Blob (see below). Self-test into v:errors.

" --- List, with a string expr (v:val) and a funcref
call assert_equal(2, indexof([10, 20, 30, 40], 'v:val == 30'))
call assert_equal(1, indexof([10, 20, 30], {i, v -> v == 20}))
call assert_equal(-1, indexof(['a', 'b'], {i, v -> v == 'z'}))

" --- v:key is the index
call assert_equal(2, indexof([5, 5, 5], 'v:key == 2'))

" --- Blob: matches a byte value
call assert_equal(1, indexof(0z0A0B0C, {i, v -> v == 11}))
call assert_equal(-1, indexof(0z0A0B0C, {i, v -> v == 255}))

" --- {startidx} begins the scan later
call assert_equal(3, indexof([1, 2, 1, 2], {i, v -> v == 2}, {'startidx': 2}))

" --- start-index edge cases differ by type (faithful to funcs.c):
"     a List uses a user index — out of range yields -1;
"     a Blob clamps a very-negative start to 0.
call assert_equal(-1, indexof([1, 2, 3], {i, v -> v == 1}, {'startidx': -99}))
call assert_equal(0, indexof(0z0A0B, {i, v -> v == 10}, {'startidx': -99}))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'indexof.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'indexof.vim: all assertions passed'
