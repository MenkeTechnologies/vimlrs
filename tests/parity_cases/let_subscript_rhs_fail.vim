" A :let whose right-hand side fails assigns nothing, also through a subscript
" (c: ex_let skips ex_let_vars on an eval0() FAIL), and a compound operator
" reports the error of reading the target once, not again for the store.
let d = {}
let d[[]] = nosuch
echo d
let d['k'] = nosuch
echo d
let d.k = 1
let d.k .= nosuch
echo d
let d[[]] .= 1
echo d
let l = [1, 2]
let l[[]] += 1
echo l
let l[5] += 1
echo l
let l[0] = [][0]
echo l
let l[0:1] = nosuch
echo l
try
  let d.k = nosuch
catch
  echo v:exception
endtry
echo d
function F()
  echo [][0]
  return 7
endfunction
let d.f = F()
echo d
