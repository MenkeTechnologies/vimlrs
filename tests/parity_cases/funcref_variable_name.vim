" set_var_const() refuses a Funcref under a name that could not be called
" (E704) or that would hide a function (E705), before any read-only or lock
" check. A :for or :let unpack whose store fails stops there.
let F = function('strlen')
try | let s = F | echo 'set' s | catch | echo v:exception | endtry
try | let g:t = F | catch | echo v:exception | endtry
try | let l = [F] | echo 'list ok' | catch | echo v:exception | endtry
try | let d = {} | let d.x = F | echo 'dict ok' | catch | echo v:exception | endtry
let b:x = F
let s:y = F
let Ok = F
let g:Ok2 = F
let auto#name = F
echo exists('s') exists('g:t') exists('b:x') exists('s:y') exists('g:Ok2')
function! Strlen2(x)
  return 1
endfunction
try | let Strlen2 = F | catch | echo v:exception | endtry
try | let Strlen = F | echo 'Strlen ok' | catch | echo v:exception | endtry
try | let strlen = F | catch | echo v:exception | endtry
try | let q = 1 | lockvar q | let q = F | catch | echo v:exception | endtry
try | let v:count = F | catch | echo v:exception | endtry
try | let a:x = F | catch | echo v:exception | endtry
try | let lam = {-> 1} | catch | echo v:exception | endtry
try | let [a, b] = [1, F] | catch | echo v:exception | endtry
echo a exists('b')
function T(x)
  let F = function('strlen')
  try | let s = F | echo 'set' | catch | echo v:exception | endtry
  try | let l:t = F | echo 'set2' | catch | echo v:exception | endtry
  try | let a:x = F | catch | echo v:exception | endtry
  try | let a:X = F | catch | echo v:exception | endtry
  for x in [1, F]
    echo 'for' x
  endfor
  echo 'after'
endfunction
try | call T(1) | catch | echo v:exception | endtry
try
  for y in [2, function('len')]
    echo y
  endfor
catch
  echo v:exception
endtry
for [m, n] in [[1, 2], [function('len'), 3]]
  echo m n
endfor
for z in [3, F] | echo z | endfor
echo 'end'
