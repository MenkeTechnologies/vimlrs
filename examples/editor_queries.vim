" editor_queries.vim — builtins whose answer is well-defined when no editor is
" attached: indent.c / fold.c / highlight_group.c / diff.c / plines.c /
" cmdexpand.c / search.c / insexpand.c.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- no buffer / fold / diff: the line/column queries report 'nothing here'
call assert_equal(-1, indent(1))
call assert_equal(0, diff_filler(1))
call assert_equal(-1, virtcol2col(0, 1, 5))

" --- no folds: fold text is empty
call assert_equal('', foldtext())
call assert_equal('', foldtextresult(1))

" --- no highlight groups defined standalone
call assert_equal(0, highlight_exists('Normal'))

" --- wildtrigger() is a no-op (no interactive command line)
call wildtrigger()

" --- searchcount(): an all-zero count snapshot (maxcount defaults to 99)
let sc = searchcount()
call assert_equal(0, sc.current)
call assert_equal(0, sc.total)
call assert_equal(0, sc.exact_match)
call assert_equal(0, sc.incomplete)
call assert_equal(99, sc.maxcount)

" --- complete_info(): an inactive insert-completion snapshot
let ci = complete_info()
call assert_equal('', ci.mode)
call assert_equal(0, ci.pum_visible)
call assert_equal([], ci.items)
call assert_equal(-1, ci.selected)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'editor_queries.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'editor_queries.vim: all assertions passed'
