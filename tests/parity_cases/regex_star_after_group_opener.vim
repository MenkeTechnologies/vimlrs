" c: `peekchr()` `case '*'` (`regexp.c`) — a `*` is a LITERAL at the start of a
" branch only when `prevchr` is a MAGIC `(`, `&` or `|`. `\(`, `\|`, `\&` and
" the very-magic `(` all produce one; `\%(` does NOT, because its `(` is read by
" `getchr()` in a dialect where `(` is not magic. So `\%(*a\)` is E866 while
" `\(*a\)` and `\v%(*a)` keep the literal star — and `\v%(` must not be folded
" into `\%(` without keeping the provenance that tells the two apart.
for p in ['\(*a\)', '\|*a', '\&*a', '\v(*a)', '\v%(*a)', '\v(%(*a))', '\%(a\|*b\)', '\%(\&*a\)', '^\(*a\)']
  try | echo p . ' ' . match('*ab', p) | catch | echo p . ' ' . v:exception | endtry
endfor
for p in ['\%(*a\)', '\%[*a]', '\v%[*a]', '\%(\%(*a\)\)']
  try | echo p . ' ' . match('*ab', p) | catch | echo p . ' ' . v:exception | endtry
endfor
" `\*` in nomagic is the multi in EVERY position, so it is misplaced after each
" branch opener — `\&` was the one this engine still let through.
for p in ['\M\(\*a\)', '\M\|\*a', '\M\&\*a', '\M\%(\*a\)', '\V\(\*a\)']
  try | echo p . ' ' . match('*ab', p) | catch | echo p . ' ' . v:exception | endtry
endfor
" ... while in magic `\*` is always the literal, whatever precedes it.
for p in ['\%(\*a\)', '\(\*a\)', '\v(\*a)', '\%[\*a]', '\M\(*a\)']
  try | echo p . ' ' . match('*ab', p) | catch | echo p . ' ' . v:exception | endtry
endfor
