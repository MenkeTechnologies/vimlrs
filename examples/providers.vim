" providers.vim — context stack, language providers, RPC and msgpack builtins.
"
" None of these subsystems exist in the standalone interpreter (no editor
" context stack, no Python/Perl provider, no RPC channels), so each returns its
" documented \"inactive\" value: an empty Dict/List, 0, \"\", or v:null. Self-checks.
"
"   vimlrs examples/providers.vim

" ── editor context stack ──
call assert_equal({}, ctxget())
call assert_equal(0, ctxsize())
call assert_equal(0, ctxpush(['regs']))
call assert_equal(0, ctxpop())

" ── misc query/no-ops ──
" islocked(): -1 when the variable does not exist, 1 locked, 0 unlocked.
call assert_equal(-1, islocked('g:nope'))
let g:il_demo = 1
call assert_equal(0, islocked('g:il_demo'))
lockvar g:il_demo
call assert_equal(1, islocked('g:il_demo'))
unlockvar g:il_demo
call assert_equal(0, last_buffer_nr())
call assert_equal('', libcall('libc.so', 'getenv', 'HOME'))
call assert_equal(0, libcallnr('libc.so', 'getpid', 0))
call assert_equal('', submatch(0))
call assert_equal([], submatch(0, 1))

" ── msgpack: nothing in, empty out ──
call assert_equal([], msgpackdump([]))
call assert_equal([], msgpackparse([]))

" ── RPC: no channels ──
call assert_equal(0, rpcnotify(1, 'event'))
call assert_equal(0, rpcrequest(1, 'method'))
call assert_equal(0, rpcstop(1))
call assert_equal(0, stdioopen({}))

" ── language providers: not available -> v:null ──
call assert_equal(v:null, py3eval('1 + 1'))
call assert_equal(v:null, perleval('1 + 1'))

" ── demo ──
echo 'python provider available?' (py3eval('1') isnot v:null)

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: providers assertions passed'
