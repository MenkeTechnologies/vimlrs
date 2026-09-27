" A subscripted :let whose base is undefined is E121 in get_lval, and nothing
" is assigned — no second error about indexing the missing value.
let zz[0] = 1
let zz.k = 1
let zz[0:1] = [1]
let g:yy[0] = 1
function! F()
  let q[1] = 2
  return exists('q')
endfunction
echo F()
