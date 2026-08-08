let s = 'a'
let s .= 'b'
echo s
let n = 5
let n += 2 | let n -= 1 | let n *= 3 | let n /= 2 | let n %= 5
echo n
let f = 1.5
let f += 1
echo f
let [a, b] = [1, 2]
echo a b
let [c; rest] = [1,2,3]
echo c rest
let d = {}
let d.k = 1
let d['j'] = 2
echo d
let l = [1,2,3]
let l[0] = 9
let l[1:2] = [8,7]
echo l
unlet d.k
echo d
let g:gv = 3
echo g:gv
echo exists('g:gv') exists('nope')
for i in range(3)
  if i == 1 | continue | endif
  echo i
endfor
let i = 0
while i < 3
  let i += 1
  if i == 2 | break | endif
endwhile
echo i
try
  throw 'boom'
catch /boom/
  echo 'caught ' . v:exception
finally
  echo 'fin'
endtry
try
  echo 1/0
catch
  echo 'div ' . v:exception
endtry
function! Add(x, y)
  return a:x + a:y
endfunction
echo Add(1,2)
let F = function('Add')
echo F(3,4)
let L = {x -> x * 2}
echo L(5)
echo map([1,2],{i,v -> v+i})
let P = function('Add',[10])
echo P(5)
echo call('Add',[1,2])
echo string(F)
echo string(L)
