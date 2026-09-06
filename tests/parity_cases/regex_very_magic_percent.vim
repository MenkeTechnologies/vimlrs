" c: `peekchr()` lists `%` among the characters that are "magic only after \v",
" so under `\v` the WHOLE `\%…` atom family is spelled without its backslash.
" Only `%(` was being translated, so `\v%[ab]` was read as a literal `%` and a
" collection, and `\v%d97` as the literal text `%d97`.
for p in ['\v%[ab]', '\v%d97', '\v%u0061', '\v%x61', '\v%o141', '\v%^a', '\v%$', '\v%1c', '\v%<2c', '\v%>1c', '\v%(a)']
  try | echo p . ' ' . match('ab', p) . ' ' . string(matchstr('ab', p)) | catch | echo p . ' ' . v:exception | endtry
endfor
" And `\%` under `\v` is the LITERAL percent (`%` is in META, so the backslash
" toggles its magicness off).
echo string(matchstr('a%b', '\v\%'))
echo string(matchstr('a%b', '\v[%]'))
