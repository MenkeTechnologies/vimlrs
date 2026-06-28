" beeps.vim — assert_beeps()/assert_nobeep(), ported from Neovim's testing.c.
" In Vim an Ex command that errors rings the bell, so vimlrs models a 'beep' as
" a command that reports an error.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- a command that errors 'beeps': assert_beeps() passes (returns 0), and the
"     errors it produced are captured, not leaked to v:errors
call assert_equal(0, assert_beeps('call no_such_function_xyz()'))
call assert_true(empty(v:errors))

" --- a clean command does not beep: assert_nobeep() passes
call assert_equal(0, assert_nobeep('let g:beeps_demo = 1'))
call assert_equal(1, g:beeps_demo)
call assert_true(empty(v:errors))

" --- the failure paths: assert_beeps() on a quiet command records an error,
"     assert_nobeep() on an erroring one does too. Capture and clear v:errors so
"     these expected failures do not fail the script.
call assert_equal(1, assert_beeps('let g:beeps_demo = 2'))
call assert_equal(1, len(v:errors))
let v:errors = []

call assert_equal(1, assert_nobeep('call no_such_function_xyz()'))
call assert_equal(1, len(v:errors))
let v:errors = []

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'beeps.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'beeps.vim: all assertions passed'
