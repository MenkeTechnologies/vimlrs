" system.vim — shelling out and reading the environment, with embedded tests.
"
" Demonstrates the OS-interaction builtins: system() (run a command, capture its
" stdout), systemlist() (output split into lines), passing input on stdin,
" v:shell_error (the exit status), and environ() (the process environment). Like
" every example here it self-checks and exits non-zero on failure.
"
" Uses only POSIX tools (echo/printf/tr), so it runs the same on Linux and macOS.
"
"   vimlrs examples/system.vim

let greeting = system('echo hello')
let ok_code = v:shell_error
let lines = systemlist('printf "one\ntwo\nthree\n"')
let shout = system('tr a-z A-Z', 'quiet')
call system('exit 7')
let code = v:shell_error
let env = environ()

" ── unit tests ──
call assert_equal("hello\n", greeting)
call assert_equal(0, ok_code)
call assert_equal(['one', 'two', 'three'], lines)
call assert_equal('QUIET', shout)
call assert_equal(7, code)
call assert_true(has_key(env, 'PATH'))
call assert_true(len(env['PATH']) > 0)
call assert_equal(type({}), type(env))

" ── demo ──
echo 'echo hello ->' systemlist('echo hello')[0]
echo 'uppercased ->' shout
echo 'PATH has' len(split(env['PATH'], ':')) 'entries'

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: system assertions passed'
