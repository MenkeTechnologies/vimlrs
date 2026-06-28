" arglist.vim — the command-line argument list (funcs.c argc/argidx/argv).
" Running outside an editor, vimlrs has no buffer arglist, so the global,
" unnamed argument list (arglistid() == 0) is the script file(s) named on the
" command line — the standalone counterpart of Vim's file arglist. This script
" is itself the sole argument, so argc() is 1 and argv(0) is this file's path.
" Self-test: asserts into v:errors, throws if any failed.

" --- this script is the one and only argument
call assert_equal(1, argc())
call assert_equal(0, argidx())
call assert_equal(0, arglistid())

" --- argv(0) is this file's path; match the basename rather than the abs path
call assert_match('arglist\.vim$', argv(0))

" --- argv() with no argument returns the whole list; -1 is the same
call assert_equal(1, len(argv()))
call assert_equal(argv(0), argv()[0])
call assert_equal(argv(), argv(-1))

" --- an out-of-range index returns the empty string, never an error
call assert_equal('', argv(5))
call assert_equal('', argv(99))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'arglist.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'arglist.vim: all assertions passed'
