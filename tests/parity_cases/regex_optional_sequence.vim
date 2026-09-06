" c: `nfa_regatom` `case '['` under `Magic('%')` (`regexp_nfa.c:1616`) — the body
" of `\%[…]` is a `for (n = 0; (c = peekchr()) != ']'; ++n) nfa_regatom()` loop,
" NOT `nfa_regpiece`. A multi inside can therefore never be a quantifier and
" always reaches nfa_regatom's "these should follow an atom" arm; an empty body
" is E70 and an unterminated one E69, both quoting the backslash the dialect
" required.
for p in ['\%[a*]', '\%[a\+b]', '\%[a\|b]', '\%[a\{2}]', '\%[a\)b]', '\%[a\@=]', '\v%[a*]', '\%[*]', 'a\%[*b]']
  try | echo p . ' ' . match('*ab', p) | catch | echo p . ' ' . v:exception | endtry
endfor
for p in ['\%[]', '\v%[]', '\M\%[]', '\%[ab', '\v%[ab']
  try | echo p . ' ' . match('*ab', p) | catch | echo p . ' ' . v:exception | endtry
endfor
" The ones that stay legal.
for p in ['\%[a\*]', '\%[a\%[bc]]', '\v%[ab]', '\%[ab]']
  try | echo p . ' ' . matchstr('abc', p) | catch | echo p . ' ' . v:exception | endtry
endfor
