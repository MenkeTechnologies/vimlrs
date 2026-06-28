" lists_dicts.vim — list and dict manipulation with the functional builtins.
"
" Demonstrates: list/dict literals, indexing & slicing, map()/filter() driven
" by string-expression bodies (v:val/v:key), reduce() with a Funcref, numeric
" sort(), and dict iteration to build a frequency table.
"
"   vimlrs examples/lists_dicts.vim

function! Add(acc, x) abort
  return a:acc + a:x
endfunction

let nums = range(1, 10)
echo 'nums       :' nums
echo 'evens      :' filter(copy(nums), 'v:val % 2 == 0')
echo 'squared    :' map(copy(nums), 'v:val * v:val')
echo 'sum        :' reduce(nums, function('Add'), 0)
echo 'sorted desc:' reverse(sort(copy(nums), 'n'))
echo 'slice 2..4 :' nums[2:4]

let user = {'name': 'ada', 'langs': ['viml', 'rust', 'c']}
echo 'keys       :' sort(keys(user))
echo 'has langs  :' has_key(user, 'langs')
echo 'lang count :' len(user['langs'])

" Build a frequency table over a list of words.
let words = ['vim', 'rust', 'vim', 'c', 'rust', 'vim']
let freq = {}
for w in words
  let freq[w] = get(freq, w, 0) + 1
endfor
echo 'frequencies:' freq
