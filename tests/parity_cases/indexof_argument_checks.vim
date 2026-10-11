" f_indexof() checks its three arguments before anything else: a List or Blob,
" a String or Funcref, an optional Dict. Each failure is an error, never -1.
for e in ["indexof('x', 'v:val')", "indexof({}, 'v:val')", "indexof(v:null, 'v:val')",
      \ "indexof('', '')", "indexof([1], 1)", "indexof([1], [])",
      \ "indexof([1], 'v:val == 1', 1)", "indexof([1], 'v:val == 1', 'x')",
      \ "indexof([1], '')", "indexof(0z0102, 'v:val == 2', {'startidx': 0})",
      \ "indexof([1, 2], {i, v -> v == 2})",
      \ "indexof([1, 2, 3], 'v:val > 1', {'startidx': -1})"]
  try
    let r = eval(e)
    echo e '=' string(r)
  catch
    echo e '!' v:exception
  endtry
endfor
