" A callback reached through v:val is called by name, like any Funcref
" variable: map(fs, 'v:val()').
let g:F = {-> 7}
echo g:F()
let s:G = {-> 8}
echo s:G()
function H(F)
  let l:K = a:F
  return [a:F(), l:K(), K()]
endfunction
echo H({-> 9})
echo map([{-> 1}, {-> 2}], 'v:val()')
echo map([{-> 1}], {i, v -> v()})
echo filter([{-> 0}, {-> 1}], 'v:val()')
let d = {'f': {-> 3}}
echo map(d, 'v:val()')
echo map([{x -> x * 2}], 'v:val(v:key + 5)')
