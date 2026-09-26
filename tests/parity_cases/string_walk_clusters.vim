" A String is walked one CHARACTER at a time — utfc_ptr2len: a base character
" plus its composing marks — by :for and by filter()/map()/foreach(), never one
" byte or one code point at a time.
echo string(filter("é", {i -> i}))
echo string(filter("aéb", {i, v -> i != 1}))
echo string(map("aé̂b", {i, v -> '<' . v . '>'}))
echo string(map("aé̂b", {i, v -> string(i)}))
echo string(filter("日本é", 'v:key != 2'))
for c in "aéb"
  echo c strlen(c)
endfor
for c in "日本"
  echo c
endfor
for c in ""
  echo 'never'
endfor
echo 'done'
