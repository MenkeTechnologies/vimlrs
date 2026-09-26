" :for [a, b] unpacks each item the way :let [a, b] = does — the same `; rest`
" target and the same count checks — and a failed unpack ends the loop.
for [a; r] in [[1, 2, 3], [4]]
  echo a r
endfor
for [a, b; r] in [[1, 2], [3, 4, 5]]
  echo a b r
endfor
for [a, b] in [[1, 2, 3]]
  echo 'never' a b
endfor
echo 'after E687'
for [a, b] in [[5, 6], [1]]
  echo a b
endfor
echo 'after E688'
