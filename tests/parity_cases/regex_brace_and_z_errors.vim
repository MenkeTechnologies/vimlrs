" \{} limits (read_limits): malformed -> E554 ({...} without the backslash under
" \v); out-of-order bounds are swapped; \z outside a syntax item is E66/E67/E867.
let s:pats = [
  \ 'a\{3,1}x', 'a\{-3,1}', 'a\{-1,3}', 'a\{', 'a\{1', 'a\{x}', 'a\{1,x}',
  \ 'a\{1\}', 'a\{,2}', 'a\{-,2}', 'a\{-}', 'a\{}',
  \ '\v(a){', '\va{1', '\va{x}', '\va{3,1}',
  \ 'a\z', 'a\z1', 'a\zx', '\z(a\)', 'a\zsa']
for s:p in s:pats
  try
    echo s:p string(matchstr('aaaax', s:p))
  catch
    echo s:p 'caught' v:exception
  endtry
endfor
