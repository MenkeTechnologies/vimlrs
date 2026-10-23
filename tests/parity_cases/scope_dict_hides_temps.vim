" Reading a scope dict whole lists only the script's variables. `:let [a, b] =`,
" `:const [c, d] =` and `:for` keep their value, list and index in hidden
" temporaries of the running scope; none of them is a key of `g:` or `l:`.
" copy()/deepcopy() copy each item with tv_copy, which unlocks it, so a
" `:const` or `:lockvar`-ed item is writable in the copy.
let [a, b] = [1, 2]
let [c2; rest] = [3, 4, 5]
const [c, d] = [6, 7]
for i in [1, 2]
endfor
for [p, q] in [[8, 9]]
endfor
echo sort(keys(g:))
echo len(g:) has_key(g:, 'a') get(g:, 'rest')
echo sort(items(g:))
for k in sort(keys(g:))
  echon k ' '
endfor
echo ''
function! F(x)
  let [m, n] = [1, 2]
  for k in [1]
  endfor
  echo sort(keys(l:)) len(l:)
  echo sort(items(l:))
endfunction
call F(9)
" copy(g:) holds the :const items unlocked
echo sort(keys(map(copy(g:), 'v:key')))
let dd = {'x': 1, 'y': 2}
lockvar dd.x
let e = copy(dd)
let e.x = 5
echo e
let f = deepcopy(dd)
let f.x = 6
echo f
try
  let dd.x = 3
catch
  echo v:exception
endtry
let l = [1, 2]
lockvar l[0]
let l2 = copy(l)
let l2[0] = 7
echo l2 l
