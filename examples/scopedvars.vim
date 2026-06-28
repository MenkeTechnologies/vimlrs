" scopedvars.vim — scoped-variable getters/setters + job/channel builtins.
"
" With no buffers, windows, or event loop, the scoped-var getters return their
" {def} argument (the documented fallback when the variable is absent), the
" setters are no-ops, and jobs/channels/sockets fail (-1) or are no-ops (0).
" Pinning these contracts lets editor/async scripts load. Self-checks.
"
"   vimlrs examples/scopedvars.vim

" ── scoped-var getters: return the {def} argument, else '' ──
call assert_equal('DEF', getbufvar(1, 'missing', 'DEF'))
call assert_equal('', getbufvar(1, 'missing'))
call assert_equal(42, getwinvar(1, 'missing', 42))
call assert_equal('t', gettabvar(1, 'missing', 't'))
call assert_equal('tw', gettabwinvar(1, 1, 'missing', 'tw'))

" ── setters: accepted no-ops ──
call assert_equal(0, setbufvar(1, 'x', 1))
call assert_equal(0, setwinvar(1, 'x', 1))
call assert_equal(0, settabvar(1, 'x', 1))
call assert_equal(0, settabwinvar(1, 1, 'x', 1))

" ── jobs / channels: no event loop ──
call assert_equal(-1, jobstart('ls'))
call assert_equal(0, jobpid(1))
call assert_equal(0, jobstop(1))
call assert_equal([], jobwait([1, 2]))
call assert_equal(0, jobresize(1, 80, 24))
call assert_equal(0, chanclose(1))
call assert_equal(0, chansend(1, 'data'))
call assert_equal(0, feedkeys('ihello'))
call assert_equal(-1, wait(100, 1))
call assert_equal(0, sockconnect('tcp', 'localhost:80'))
call assert_equal('', win_execute(1, 'echo'))
call assert_equal(0, bufadd('/tmp/x'))

" ── demo: a config-style fallback read ──
echo 'tabstop (default):' getbufvar(1, '&tabstop', 8)

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: scopedvars assertions passed'
