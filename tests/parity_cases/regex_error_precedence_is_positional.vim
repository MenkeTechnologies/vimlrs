" Vim reports the FIRST violation in pattern order. The nomagic "misplaced star"
" is found in a pre-pass that runs before the parser, and it used to win
" unconditionally — so a backreference or `\ze` error EARLIER in the pattern was
" reported as `E866: Misplaced *`.
for s:p in ['\M\1\*', '\M\*\1', '\V\1\(\*', '\V\(\1\*', '\M\ze\{2}\*', '\M\*\ze\{2}', '\M\ze\{2}\(\*', '\M\)\*', '\M\(\*']
  try
    call match('', s:p)
  catch
    echo s:p . ' -> ' . substitute(v:exception, '^Vim(\w*):', '', '')
  endtry
endfor
