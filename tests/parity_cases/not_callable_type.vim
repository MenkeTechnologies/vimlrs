" A call through a variable that holds no function is E1085, not E117.
let F = 0
call F()
let G = 'len'
echo G('abc')
let g:H = [1]
echo g:H()
let s:x = 1
echo s:x()
function! Local()
  let L = 5
  return L()
endfunction
echo Local()
echo NoSuchFunc()
