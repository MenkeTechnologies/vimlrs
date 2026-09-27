" A quote inside an interpolated string's {expr} belongs to the expression: it
" neither ends the literal nor turns a later `"` into a comment or a `|` into a
" command separator.
let w = $'{"x'y"}'
echo w
let v = $'a {'b' .. "c"} d' " a real comment
echo v
echo $'{'a|b'}' | echo 'next'
let d = $"{"q"} \" {'r'}"
echo d
if $'{'1'}' == '1' | echo 'if ok' | endif
let l = [$'{"x'y"}', $'{{lit}}']
echo l
