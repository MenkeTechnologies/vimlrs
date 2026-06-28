" completion.vim — getcompletion() (real environment/file completion) plus the
" input/indent/completion/menu queries that have a defined value with no editor
" attached (getchar.c / indent.c / insexpand.c / cmdexpand.c / menu.c).
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- getcompletion('…', 'environment') matches environment variable names
call setenv('VIMLRS_COMPLETION_TESTVAR', 'yes')
let env = getcompletion('VIMLRS_COMPLETION_', 'environment')
call assert_true(index(env, 'VIMLRS_COMPLETION_TESTVAR') >= 0)

" --- a prefix that matches nothing yields an empty list
call assert_equal([], getcompletion('zz_definitely_no_such_prefix_zz', 'environment'))

" --- an unsupported completion type yields an empty list
call assert_equal([], getcompletion('x', 'this_is_not_a_real_type'))

" --- getcompletion('', 'file') lists the current directory (non-empty repo)
call assert_true(len(getcompletion('', 'file')) > 0)

" --- no input available standalone
call assert_equal(0, getchar(0))
call assert_equal('', getcharstr(0))
call assert_equal(0, getcharmod())

" --- no buffer: the language-indent helpers report -1
call assert_equal(-1, cindent(1))
call assert_equal(-1, lispindent(1))

" --- insert-completion is inactive
call assert_equal(0, complete_add('x'))
call assert_equal(0, complete_check())
call assert_equal({}, cmdcomplete_info())

" --- no menus defined standalone
call assert_equal({}, menu_info('File'))

" --- the test hooks are no-ops (Rust ownership, nothing to collect/log)
call test_garbagecollect_now()
call test_write_list_log('tag')

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'completion.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'completion.vim: all assertions passed'
