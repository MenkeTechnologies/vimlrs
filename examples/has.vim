" has.vim — feature detection (has(), funcs.c). vimlrs reports only what it
" genuinely is: the real build platform (like Neovim's #ifdef-gated has_list[])
" plus the language features it actually implements. Editor features Neovim
" claims — windows, syntax, folding, mouse, statusline — are absent here, and
" vimlrs is neither Vim nor Nvim, so version/patch and 'nvim' are absent too.
" Self-test: asserts into v:errors, throws if any failed.

" --- language/runtime features vimlrs implements
call assert_equal(1, has('eval'))
call assert_equal(1, has('float'))
call assert_equal(1, has('vimlrs'))
call assert_equal(1, has('lambda'))
call assert_equal(1, has('num64'))
call assert_equal(1, has('reltime'))
call assert_equal(1, has('iconv'))
call assert_equal(1, has('digraphs'))

" --- syntax-version features: vimlrs runs both legacy Vim script and Vim9
" script (var/def/typed params), so it reports vimscript-1 and vim9script,
" matching real Vim 9.2 which returns 1 for both.
call assert_equal(1, has('vimscript-1'))
call assert_equal(1, has('vim9script'))
call assert_equal(has('vim9script'), has('VIM9SCRIPT'))

" --- feature names are matched case-insensitively (Vim uses STRICMP)
call assert_equal(has('eval'), has('EVAL'))
call assert_equal(has('unix'), has('Unix'))

" --- not Vim/Nvim, no GUI, no editor subsystems, unknown features -> 0
call assert_equal(0, has('nvim'))
call assert_equal(0, has('gui_running'))
call assert_equal(0, has('syntax'))
call assert_equal(0, has('windows'))
call assert_equal(0, has('folding'))
call assert_equal(0, has('patch-8.1.0'))
call assert_equal(0, has('a_feature_that_does_not_exist'))

" --- exactly one platform family is reported, and macOS implies unix
call assert_equal(1, has('unix') || has('win32'))
if has('mac')
  call assert_equal(1, has('unix'))
  call assert_equal(1, has('macunix'))
  call assert_equal(1, has('osx'))
endif
if has('win32')
  call assert_equal(0, has('unix'))
endif

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'has.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'has.vim: all assertions passed'
