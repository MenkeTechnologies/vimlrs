" A name read before anything assigned it is E121, even where the name is
" otherwise a plain Number local: `let n += 1` with no `n`.
let zz += 1
echo exists('zz')
function! F()
  let n += 1
  return exists('n')
endfunction
echo F()
let i = 0
let s = 0
while i < 5
  let s += i
  let i += 1
endwhile
echo s i
let w = 0
while w < 3
  if w == 1
    let late = 7
  endif
  let w += 1
endwhile
echo late
