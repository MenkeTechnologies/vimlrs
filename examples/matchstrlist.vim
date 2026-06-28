" matchstrlist.vim — matchstrlist() returns every match across a List of strings
" (funcs.c get_matches_in_str). Each result is a Dict {idx, byteidx, text}, plus
" a {submatches} list when the {dict} option asks for it. Crucially it reports
" ALL matches in each string, not just the first. Self-test into v:errors.

" --- multiple matches within a single item are all reported
call assert_equal(
      \ [{'idx': 0, 'byteidx': 1, 'text': 'X'}, {'idx': 0, 'byteidx': 3, 'text': 'X'}],
      \ matchstrlist(['aXbXc'], 'X'))

" --- matches across several items carry their item index and byte offset
call assert_equal(
      \ [{'idx': 0, 'byteidx': 1, 'text': '1'},
      \  {'idx': 0, 'byteidx': 3, 'text': '2'},
      \  {'idx': 1, 'byteidx': 1, 'text': '3'}],
      \ matchstrlist(['a1b2', 'c3'], '\d'))

" --- {submatches: v:true} adds the \1..\9 group list (9 slots)
call assert_equal(
      \ [{'idx': 0, 'byteidx': 1, 'text': 'oo',
      \   'submatches': ['o', 'o', '', '', '', '', '', '', '']}],
      \ matchstrlist(['foobar'], '\(o\)\(o\)', {'submatches': v:true}))

" --- no matches → empty List
call assert_equal([], matchstrlist(['abc', 'def'], 'z'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'matchstrlist.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'matchstrlist.vim: all assertions passed'
