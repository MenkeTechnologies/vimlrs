" sort() with a {func} name: an unknown function is E117 before E702, and a
" builtin works as the compare function (call_func reaches both).
echo string(sort([1, 2], 'nosuch'))
echo string(sort(['bb', 'a', 'ccc'], {a, b -> len(a) - len(b)}))
function! Cmp(a, b)
  return a:a - a:b
endfunction
echo string(sort([3, 1], 'Cmp')) string(sort([3, 1], function('Cmp')))
echo string(sort([[3], [1]], 'max'))
echo string(uniq([1, 1, 2], 'Cmp'))
