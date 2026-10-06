" `:for` over each iterable type. c: eval_for_line()/next_for_item()
" (vendor/eval.c:1435, :1511).
"
" A String yields one item per CHARACTER, utfc_ptr2len bytes each, so a
" composing character stays with its base; it used to be walked byte by byte
" (`<c3>`, `<a9>`). A Blob is copied first and yields Numbers. An item that
" fails the `[a, b; rest]` target count ends the loop with nothing assigned
" from it. A non-iterable is an error and no iteration — the error TEXT is an
" engine split (vim: E1523 "String, List, Tuple or Blob required"; Neovim,
" whose C this port follows: E1098 "String, List or Blob required"), so this
" case catches either and checks that the body never runs.

for c in 'aéb'
  echon c '|'
endfor
echo ''
for c in "éx"
  echo c strlen(c)
endfor
for c in ''
  echo 'never'
endfor
let s = 'xy'
for c in s
  let s = 'changed'
  echo c
endfor
echo s

let b = 0z010203
for x in b
  let b[2] = 9
  echo x
endfor
echo b
for x in 0z
  echo 'never'
endfor

let l = [1, 2, 3]
for x in l
  echo x
endfor

for [a, b] in [[1, 2], [3, 4]]
  echo a b
endfor
for [a; r] in [[1, 2, 3], [4]]
  echo a r
endfor
for [a, b; r] in [[1, 2], [3, 4, 5]]
  echo a b r
endfor
for [a, b] in [[1, 2], [3], [5, 6]]
  echo a b
endfor
echo 'after short item'
for [a, b] in [[1, 2], [3, 4, 5], [7, 8]]
  echo a b
endfor
echo 'after long item'

try
  for x in 5
    echo 'never'
  endfor
catch /E1098\|E1523/
  echo 'not iterable'
endtry
echo 'done'
