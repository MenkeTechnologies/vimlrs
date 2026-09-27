" sort() and uniq() call a Partial comparator with its bound arguments and
" closure scope (item_compare_partial), and a {dict} as its self.
function! Cmp(sign, a, b)
  return a:sign * (a:a - a:b)
endfunction
echo sort([3, 1, 2], function('Cmp', [1]))
echo sort([3, 1, 2], function('Cmp', [-1]))
echo uniq([1, 1, 2], function('Cmp', [1]))
let d = {'m': -1}
function! Dcmp(a, b) dict
  return self.m * (a:a - a:b)
endfunction
echo sort([3, 1, 2], 'Dcmp', d)
function! ByOrder()
  let order = [3, 1, 2]
  return sort([1, 2, 3], {a, b -> index(order, a) - index(order, b)})
endfunction
echo ByOrder()
