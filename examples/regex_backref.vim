" regex_backref.vim — backreferences \1..\9 in the search pattern (regexp).
" A backreference matches the exact text a previous capture group matched, so
" \(.\)\1 finds a doubled character. Used by matchstr/substitute/=~. An unset
" group's backreference matches the empty string. Self-test into v:errors.

" --- \1 matches what group 1 captured (a doubled letter)
call assert_equal('ll', matchstr('hello', '\(l\)\1'))
call assert_equal('abcabc', matchstr('abcabc', '\(abc\)\1'))
call assert_equal(1, 'mississippi' =~ '\(.\)\1')
call assert_equal(0, 'abcdef' =~ '\(.\)\1')

" --- substitute() honours a backreference in the pattern
call assert_equal('heXo', substitute('hello', '\(l\)\1', 'X', ''))
call assert_equal('a-b', substitute('aa-bb', '\(\a\)\1', '\1', 'g'))

" --- groups still capture normally (no regression)
call assert_equal(['2024-01', '2024', '01'], matchlist('2024-01', '\(\d\+\)-\(\d\+\)')[0:2])

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'regex_backref.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'regex_backref.vim: all assertions passed'
