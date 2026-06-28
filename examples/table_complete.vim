" table_complete.vim — the last builtins that complete the eval.lua function
" table: provider evals (no provider), mouse/screen positions, completion/script
" introspection, the deprecated job aliases, and the insert-mode no-ops.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- no Python provider: pyeval()/pyxeval() return v:null
call assert_equal(v:null, pyeval('1 + 1'))
call assert_equal(v:null, pyxeval('"x"'))

" --- no mouse / screen standalone: all-zero position dicts
call assert_equal(0, getmousepos().line)
call assert_equal(0, getmousepos().screenrow)
call assert_equal(0, screenpos(0, 1, 1).row)
call assert_equal(0, screenpos(0, 1, 1).endcol)

" --- nothing to introspect standalone
call assert_equal([], getscriptinfo())
call assert_equal([], getstacktrace())
call assert_equal('', getcompletiontype('ec'))
call assert_equal('', preinserted())

" --- insert-mode completion / mapping restore are no-ops standalone
call complete(1, ['a', 'b'])
call mapset('n', 0, {})

" --- jobsend()/jobclose() are the deprecated aliases of chansend()/chanclose();
"     with no channel they report 'nothing sent'/'closed'
call assert_equal(0, jobsend(0, 'data'))
call assert_equal(0, jobclose(0))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'table_complete.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'table_complete.vim: all assertions passed'
