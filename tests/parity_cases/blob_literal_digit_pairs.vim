" eval_number()'s 0z branch: hex digits are read in PAIRS, a '.' is skipped only
" between two pairs, and an odd digit is E973 — raised at evaluation time, after
" the operands to its left have run.
function! Side()
  echo 'side'
  return 1
endfunction
for e in ["0z0", "0z012", "0z01.0", "0z0.", "0z01.2", "[0z01, 0z2]",
      \ "0z01.02", "0zAB.cd.EF", "len(0z00.11.22)", "0z0102 == 0z01.02",
      \ "0z.01", "0z01..02", "0zG", "0z"]
  try
    let r = eval(e)
    echo e '=' string(r)
  catch
    echo e '!' v:exception
  endtry
endfor
try
  let x = Side() + 0z0
catch
  echo v:exception
endtry
let y = 0z012
echo exists('y')
