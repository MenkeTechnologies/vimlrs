" windows.vim — buffer-line and window/tab query builtins, with unit tests.
"
" A standalone interpreter has no buffer lines, windows, or GUI, so these return
" the documented \"absent\" values: reading a line gives \"\" / [], a line-changing
" command FAILs with 1, window queries give no id (0) / -1 / [] / [0,0], and the
" GUI position is [-1,-1]. Pinning these contracts lets editor-oriented scripts
" load and degrade gracefully instead of crashing. Self-checks.
"
"   vimlrs examples/windows.vim

" ── reading buffer lines ──
call assert_equal('', getline(1))
call assert_equal([], getline(1, 5))
call assert_equal([], getbufline(1, 1, '$'))
call assert_equal('', getbufoneline(1, 1))
call assert_equal([], getbufinfo())

" ── line-changing commands FAIL (return 1) with no buffer ──
call assert_equal(1, setline(1, 'x'))
call assert_equal(1, append(1, 'x'))
call assert_equal(1, setbufline(1, 1, 'x'))
call assert_equal(1, appendbufline(1, 1, 'x'))
call assert_equal(1, deletebufline(1, 1))

" ── window / tab queries ──
call assert_equal([], getwininfo())
call assert_equal([], gettabinfo())
call assert_equal([-1, -1], getwinpos())
call assert_equal(-1, getwinposx())
call assert_equal(-1, getwinposy())
call assert_equal(0, win_getid())
call assert_equal(0, win_id2win(1000))
call assert_equal([], win_findbuf(1))
call assert_equal(0, win_gotoid(1000))
call assert_equal('unknown', win_gettype())
call assert_equal([0, 0], win_screenpos(1))

" ── demo: a guard that adapts to the headless runtime ──
if win_getid() == 0
  echo 'running headless — no window to act on'
endif

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: windows assertions passed'
