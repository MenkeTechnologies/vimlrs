" E65 is raised by the BACKTRACKING engine (`regexp_bt.c` `seen_endbrace`), which
" the automatic engine falls back to once the NFA compile fails — and that
" function carries an explicit escape hatch: "Trick: check if \"@<=\" or \"@<!\"
" follows, in which case the \1 can appear before the referenced match". It is a
" plain forward scan of the remaining pattern TEXT, not a parse.
for p in ['\1@<=', '\2@<!', '\1', '\1a', '\2', '\9', '\v\1', '\M\1', '\(a\1\)', '\%(a\)\1']
  try | echo p . ' ' . match('a', p) | catch | echo p . ' ' . v:exception | endtry
endfor
for p in ['\(a\)\1', '\(\(a\)\2\)', '\(a\)\|\1', '\(a\)\2']
  try | echo p . ' ' . match('a', p) | catch | echo p . ' ' . v:exception | endtry
endfor
" The scan starts at the backreference: a lookbehind BEHIND it does not excuse
" one that follows.
try | echo match('a', '\1x\@<=') | catch | echo v:exception | endtry
try | echo match('a', '\(a\)\@<=\1') | catch | echo v:exception | endtry
