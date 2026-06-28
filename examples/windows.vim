" windows.vim — window/tab query builtins, with unit tests.
"
" A standalone interpreter has no windows, tab pages, or GUI, so these return
" the documented \"absent\" values: window queries give no id (0) / -1 / [] /
" [0,0], and the GUI position is [-1,-1]. (The in-memory buffer itself is real —
" see buffer.vim.) Pinning these contracts lets editor-oriented scripts load and
" degrade gracefully instead of crashing. Self-checks.
"
"   vimlrs examples/windows.vim

" ── the virtual buffer is real: a fresh buffer reads as one empty line, and
"    line-changing commands succeed (return 0) ──
call assert_equal('', getline(1))
call assert_equal([''], getline(1, 5))
call assert_equal([''], getbufline(1, 1, '$'))
call assert_equal('', getbufoneline(1, 1))
" getbufinfo() describes the single virtual buffer (number 1)
call assert_equal(1, getbufinfo()[0].bufnr)
call assert_equal(1, getbufinfo()[0].linecount)
call assert_equal(0, setline(1, 'x'))
call assert_equal(0, append(1, 'x'))
call assert_equal(0, setbufline(1, 1, 'x'))
call assert_equal(0, appendbufline(1, 1, 'x'))
call assert_equal(0, deletebufline(1, 1))

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
