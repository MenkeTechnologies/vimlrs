" `:for` with a curly-braces loop variable: the name is expanded for each
" item (next_for_item -> ex_let_vars -> get_lval -> make_expanded_name).
let n = 'x'
for {n} in [1, 2]
  echo x
endfor
let m = 'k'
for j{m} in [3]
  echo jk
endfor
for g:{n}y in [7]
endfor
echo g:xy
" Re-expanded per item: the body can change what the name expands to.
let p = 'a'
for {p} in ['b', 'c']
  let p = 'q'
endfor
echo a q
" A String is walked one character at a time; break/continue as usual.
for {m} in range(5)
  if k == 1 | continue | endif
  if k == 3 | break | endif
  echo k
endfor
for {n} in 'ab'
  echo x
endfor
" A Funcref item needs a capital name; the expanded one has it.
let n = 'X'
for {n} in [function('len')]
endfor
echo X('abc')
function F()
  let n = 'v'
  for {n} in [1, 2]
    echo v
  endfor
  for l:{n}2 in [4]
    echo v2
  endfor
endfunction
call F()
" A failed {expr}: E121, then E475 quoting the argument (comment included),
" and the loop ends.
for {nosuch} in [1, 2]   " cmt
  echo 'body'
endfor
echo 'after'
for {nosuch} in []
endfor
