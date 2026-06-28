" lists_dicts.vim — list/dict manipulation, with embedded unit tests.
"
" Demonstrates: list/dict literals, indexing & slicing, map()/filter() with
" string-expression bodies, reduce() with a Funcref, numeric sort(), dict
" iteration, and the built-in assert framework. Exits non-zero on failure.
"
"   vimlrs examples/lists_dicts.vim

function! Add(acc, x) abort
  return a:acc + a:x
endfunction

let nums = range(1, 10)
let evens = filter(copy(nums), 'v:val % 2 == 0')
let squared = map(copy(nums), 'v:val * v:val')
let total = reduce(nums, function('Add'), 0)
let desc = reverse(sort(copy(nums), 'n'))

let user = {'name': 'ada', 'langs': ['viml', 'rust', 'c']}

let words = ['vim', 'rust', 'vim', 'c', 'rust', 'vim']
let freq = {}
for w in words
  let freq[w] = get(freq, w, 0) + 1
endfor

" ── unit tests ──
call assert_equal([2, 4, 6, 8, 10], evens)
call assert_equal([1, 4, 9, 16, 25, 36, 49, 64, 81, 100], squared)
call assert_equal(55, total)
call assert_equal([10, 9, 8, 7, 6, 5, 4, 3, 2, 1], desc)
call assert_equal([3, 4, 5], nums[2:4])
call assert_equal(['langs', 'name'], sort(keys(user)))
call assert_true(has_key(user, 'langs'))
call assert_equal(3, len(user['langs']))
call assert_equal({'vim': 3, 'rust': 2, 'c': 1}, freq)
call assert_equal(3, freq['vim'])

" ── demo ──
echo 'nums       :' nums
echo 'evens      :' evens
echo 'squared    :' squared
echo 'sum        :' total
echo 'sorted desc:' desc
echo 'frequencies:' freq

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: lists_dicts assertions passed'
