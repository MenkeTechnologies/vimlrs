" eval0() evaluates what parsed before it complains about trailing text: the
" expression's own error wins, and its side effects happen.
function! F()
  echo 'called'
  return 1
endfunction
let a = F() x
echo exists('a')
if F() x
endif
let b = nosuch )
let c = [][0] x
let x1 = 1
let x2 = x1 = 3
echo exists('x2')
" :let op= evaluates {expr} before it reads the target.
let n += nosuch
let n += 1 2
let m = 1
let m += nosuch x
echo m
" A failed {expr} leaves an unpack alone (no E714 after it).
let [p, q] = [1, 2] 3
let [p, q] = nosuch
echo exists('p')
" An index that cannot be a String is one error.
let dd = {}
let dd[[]] = 1
let dd[{}] = 1
let ll = [1]
let ll[[]] = 1
echo dd[[]]
