" c: `nfa_regatom` `case Magic('%')` — the codepoint atoms read through
" `getdecchrs`/`getoctchrs`/`gethexchrs`, each with its own digit budget:
" decimal is UNBOUNDED, octal stops after three digits OR once the value reaches
" 040, hex takes 2/4/8. An empty run or a value over INT_MAX is E678, where this
" engine used to fall back to the literal radix letter.
for c in [['\%o101','A'], ['\%o1011','A1'], ['\%o40',' '], ['\%o401',' 1'], ['\%d0000000097','a'], ['\%u00610','a0'], ['\%x610','a0']]
  echo c[0] . ' ' . string(matchstr(c[1], c[0]))
endfor
for p in ['a\%d', 'a\%da', 'a\%xzz', 'a\%o9', '\%d999999999999', '\v%d']
  try | echo p . ' ' . match('a', p) | catch | echo p . ' ' . v:exception | endtry
endfor
" The position family, its cursor-relative `\%.c` form, the mark forms, and the
" E867 that catches everything else. An unrecognized `\%x` used to become a
" literal `%` and match nothing quietly.
for p in ['a\%q', 'a\%', 'a\%23x', 'a\%<2', 'a\%2', 'a\%l', 'a\%c', 'a\%v', 'a\%.5c', 'a\%2''z', 'a\%2147483648c']
  try | echo p . ' ' . match('abc', p) | catch | echo p . ' ' . v:exception | endtry
endfor
" Recognized, and unsatisfiable in a string: no cursor, no Visual area, no mark.
for p in ['a\%#b', 'a\%Vb', 'a\%.c', 'a\%<.c', 'a\%''mb', 'a\%<''m', 'a\%1lb']
  try | echo p . ' ' . match('abc', p) | catch | echo p . ' ' . v:exception | endtry
endfor

" c: `nfa_reg` — `if (regnpar >= NSUBEXP) EMSG_RET_FAIL(e_nfa_regexp_too_many_parens)`
" with `regnpar` starting at 1 and NSUBEXP 10: nine capture groups are the
" limit. Non-capturing groups are not counted.
for n in [8, 9, 10, 11]
  let p = repeat('\(a\)', n)
  try | echo n . ' ' . match(repeat('a', 11), p) | catch | echo n . ' ' . v:exception | endtry
endfor
echo 'nc10 ' . match(repeat('a', 11), repeat('\%(a\)', 10))
try | echo 'nest ' . match('a', repeat('\(', 10) . 'a' . repeat('\)', 10)) | catch | echo 'nest ' . v:exception | endtry

" A `\m` restores the default dialect mid-pattern and must be REMOVED even when
" the pattern never left it — it was reaching the parser as an atom, so
" `matchend('X', '.\m\%[ab]')` looked for a literal `m`.
echo matchend('日', '\C\(\)\?.\m\%[ab]')
echo matchend('日', '.\m\%[ab]')
echo string(matchstr('amb', 'a\mb'))
echo string(matchstr('ab', 'a\mb'))
