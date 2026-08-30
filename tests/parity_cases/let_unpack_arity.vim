" `:let [a, b] = list` — the target-count rules, which are checked BEFORE any
" name is assigned, and the two errors that say which way the count is wrong.
let [a, b] = [1, 2]
echo a b
" `; rest` takes whatever is left, including nothing at all.
let [x, y; r] = [1, 2, 3, 4]
echo x y r
let [p, q; r2] = [1, 2]
echo p q r2
" Fewer items than targets is E688, with and without a rest name.
try
  let [u, v] = [1]
catch
  echo v:exception
endtry
try
  let [u2, v2; r3] = [1]
catch
  echo v:exception
endtry
" More items than targets is E687 — but only when there is NO rest name, since
" a rest absorbs the surplus.
try
  let [w, z] = [1, 2, 3]
catch
  echo v:exception
endtry
" Nothing is assigned when the count is wrong: these names stay unset.
echo exists('u') exists('v') exists('w') exists('z')
" The unpack also reads from a function's return and from split().
function! Two()
  return [10, 20]
endfunction
let [f1, f2] = Two()
echo f1 f2
let [k, val] = split('key=value', '=')
echo k val
" A single-target unpack is still a list unpack, not a plain assignment.
let [only] = [42]
echo only
try
  let [only2] = [1, 2]
catch
  echo v:exception
endtry
