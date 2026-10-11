" eval_expr_typval(): anything that is not a Funcref/Partial is read with
" tv_get_string_buf_chk() and evaluated as an expression, so a Number, Float or
" Bool {expr} is its own text; a List/Dict/Blob is that conversion's error.
for e in ["map([1, 2], 1)", "map([1, 2], 7)", "map({'a': 1}, 2.5)",
      \ "map([1], v:true)", "map([1], v:null)", "mapnew([0, 0], 3)",
      \ "filter([1, 2], 1)", "filter([1, 2], 0)", "filter(0z0102, 1)",
      \ "indexof([5, 6], 1)", "map('ab', 1)",
      \ "map([1, 2], [])", "map([1], {})", "filter([1], 0z01)"]
  try
    let r = eval(e)
    echo e '=' string(r)
  catch
    echo e '!' v:exception
  endtry
endfor
