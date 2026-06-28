" slicing.vim — slice() and character-aware string utilities, with unit tests.
"
" Demonstrates slice() (an exclusive-end slice of a List/String/Blob, with
" character indices for strings and negative indices), strcharlen() (character
" count that folds composing marks into their base), and strtrans() (render
" unprintable characters as ^X). Self-checks and exits non-zero on failure.
"
"   vimlrs examples/slicing.vim

let nums = [0, 1, 2, 3, 4, 5]

" ── slice(): exclusive end, unlike the inclusive [a:b] index form ──
call assert_equal([1, 2], slice(nums, 1, 3))
call assert_equal([2, 3, 4, 5], slice(nums, 2))
call assert_equal([1, 2, 3, 4], slice(nums, 1, -1))
call assert_equal([4, 5], slice(nums, -2))
call assert_equal([], slice(nums, 3, 1))
" Contrast: the [a:b] index form includes b.
call assert_equal([1, 2, 3], nums[1:3])

" slice() on a String uses character indices.
call assert_equal('ell', slice('hello', 1, 4))
call assert_equal('llo', slice('hello', 2))

" slice() on a Blob uses byte indices.
call assert_equal(list2blob([17, 34]), slice(list2blob([0, 17, 34, 51]), 1, 3))

" ── strcharlen(): folds a composing mark into its base character ──
let accented = 'e' . nr2char(769)
call assert_equal(1, strcharlen(accented))
call assert_equal(2, strchars(accented))
call assert_equal(5, strcharlen('hello'))

" ── strtrans(): unprintable -> printable ──
call assert_equal('a^Ib', strtrans("a\tb"))
call assert_equal('^[done', strtrans(nr2char(27) . 'done'))

" ── demo ──
echo 'slice(nums,1,4) ->' slice(nums, 1, 4)
echo 'slice(hello,0,3)->' slice('hello', 0, 3)
echo 'strtrans(tab)   ->' strtrans("x\ty")

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: slicing assertions passed'
