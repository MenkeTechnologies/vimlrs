" regex_optseq.vim — the \%[atoms] optional-sequence atom (regexp).
" \%[abc] matches a greedy in-order prefix of its atoms — '', 'a', 'ab', or 'abc'
" — each atom matched only if the previous one was. It is the idiom for
" abbreviatable keywords, e.g. r\%[ead] matches 'r','re','rea','read'. Patterns
" use single quotes so the backslashes are literal. Self-test into v:errors.

" --- a command name with an optional tail
call assert_equal('func', matchstr('function', 'f\%[unc]'))
call assert_equal('fun', matchstr('fun', 'f\%[unc]'))
call assert_equal('f', matchstr('f', 'f\%[unc]'))

" --- the classic abbreviatable-keyword use
call assert_equal('read', matchstr('read', 'r\%[ead]'))
call assert_equal('rea', matchstr('rea', 'r\%[ead]'))
call assert_equal('r', matchstr('r', 'r\%[ead]'))

" --- the optional run stops at the first atom that does not match
call assert_equal('func', matchstr('func and tion', 'func\%[tion]'))

" --- it participates in a larger pattern and in =~
call assert_equal(1, 'substitute' =~ 's\%[ubstitute]')
call assert_equal('set', matchstr('set', 's\%[et]'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'regex_optseq.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'regex_optseq.vim: all assertions passed'
