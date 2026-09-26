" :const is `ex_let` with is_const: the value is evaluated first, then each
" target that already exists is E995 (and the assignment stops there), and each
" new one is bound and locked. A list target unpacks, counts and locks like
" :let [a, b; rest] = does.
const c = 1
try
  const c = 2
catch
  echo v:exception
endtry
echo c
try
  const c = Nope()
catch
  echo v:exception
endtry
const [a, b; r] = [1, 2, 3, 4]
echo a b r
try
  let a = 5
catch
  echo v:exception
endtry
try
  call add(r, 5)
catch
  echo v:exception
endtry
let x = 1
try
  const [x2, x] = [7, 8]
catch
  echo v:exception
endtry
echo x2 x
try
  const [p, q] = [1]
catch
  echo v:exception
endtry
echo exists('p')
const d = {'k': [1]}
try
  call add(d.k, 2)
catch
  echo v:exception
endtry
function F()
  const z = 3
  try
    const z = 4
  catch
    echo v:exception
  endtry
  return z
endfunction
echo F()
const c | echo 'bar'
