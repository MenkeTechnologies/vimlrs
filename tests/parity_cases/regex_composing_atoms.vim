" R41-O2: `\%C` is NFA_ANY_COMPOSING — it skips over a composing character
" when there is one at this position and matches without consuming otherwise.
" And `utf_class_buf()` ends with "most other characters are word characters"
" for every codepoint at or above 0x100 that is not in its punctuation table,
" which includes the combining marks — so `\<` matches in front of one.
let s = "́b"
let t = "áb"
echo match(s, '\<')
echo match(s, '\w')
echo '<' . matchstr(t, 'a\%Cb') . '>'
echo '<' . matchstr(t, 'a\%C') . '>'
echo '<' . matchstr(t, '.') . '>'
echo '<' . matchstr(t, 'a') . '>'
echo match(t, 'b')
echo split(s, '\<')
echo split(t, '\zs')
echo split(t, '\<')
echo strchars(matchstr(t, '\%Ca'))
