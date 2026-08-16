" fill_assert_error()'s Dict narrowing — when assert_equal() is given two Dicts
" it reports only the keys that DIFFER and says how many equal ones it left out,
" so a one-key difference in a wide Dict is not buried in the dump.
function! Msg()
  let e = v:errors[0]
  let v:errors = []
  return substitute(e, '^.\{-}line \d\+: ', '', '')
endfunction

" One key differs out of two.
call assert_equal({'a': 1, 'b': 2}, {'a': 1, 'b': 3})
echo Msg()

" Plural, and the count is of the OMITTED keys, not the reported ones.
call assert_equal({'a': 1, 'b': 2, 'c': 3}, {'a': 1, 'b': 2, 'c': 4})
echo Msg()

" Nothing in common: nothing is omitted and both sides print whole. The key only
" the expected side has appears on the left, the got-only key on the right.
call assert_equal({'a': 1}, {'b': 1})
echo Msg()

" A key missing from the got side is a difference, so it is kept — and the got
" side simply has no entry for it.
call assert_equal({'a': 1, 'b': 2}, {'a': 1})
echo Msg()

" A key only the got side has is added by the second pass.
call assert_equal({'a': 1}, {'a': 1, 'b': 2})
echo Msg()

" Values compare with tv_equal, so a nested container counts as equal when its
" contents are.
call assert_equal({'a': [1, 2], 'b': 1}, {'a': [1, 2], 'b': 2})
echo Msg()

" assert_notequal has no "got" side to diff against, so it reports the whole
" value.
call assert_notequal({'a': 1, 'b': 2}, {'a': 1, 'b': 2})
echo Msg()

" Only a Dict/Dict pair narrows: a Dict against a List prints both in full.
call assert_equal({'a': 1}, [1])
echo Msg()
