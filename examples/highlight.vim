" highlight.vim — highlight-group and syntax queries whose answer is well
" defined outside an editor: with no loaded syntax/highlight tables every group
" lookup misses. hlID() (funcs.c) and its deprecated alias highlightID() both
" resolve a group name to its numeric id (0 when undefined); diff_hlID() (diff.c)
" reports the diff-mode highlight id at a position (0 with no diff change). The
" idiomatic chain hlID()->synIDtrans()->synIDattr() therefore folds to "".
" Self-test: asserts into v:errors, throws if any failed.

" --- no highlight groups: every name resolves to id 0
call assert_equal(0, hlID('Comment'))
call assert_equal(0, hlID('Normal'))
call assert_equal(0, hlID('ThisGroupDoesNotExist'))

" --- highlightID() is the deprecated alias of hlID(): identical answer
call assert_equal(hlID('Comment'), highlightID('Comment'))

" --- hlexists() agrees there is no such group
call assert_equal(0, hlexists('Comment'))

" --- no diff mode: diff_hlID() reports id 0 at any line/column
call assert_equal(0, diff_hlID(1, 1))
call assert_equal(0, diff_hlID('.', 5))

" --- the documented chain to read a group's attribute folds cleanly to ""
call assert_equal('', synIDattr(synIDtrans(hlID('Comment')), 'bg'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'highlight.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'highlight.vim: all assertions passed'
