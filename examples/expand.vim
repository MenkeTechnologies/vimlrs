" expand.vim — expand() / expandcmd() plus more headless-window builtins.
"
" expand() resolves $VAR and ~ and file wildcards; editor specials (%, #, <…>)
" have no current file standalone and expand to \"\". The window-view / prompt /
" server builtins return their documented \"inactive\" values. Self-checks.
"
"   vimlrs examples/expand.vim

" ── expand() ──
call assert_equal($HOME, expand('$HOME'))
call assert_equal($HOME, expand('~'))
call assert_equal($HOME . '/x', expand('~/x'))
call assert_equal('', expand('%'))
call assert_equal('', expand('<cword>'))
" Wildcards expand to matching files (List form).
call assert_true(len(expand('examples/*.vim', 0, 1)) >= 10)
" expandcmd() expands $VAR inside a command string.
call assert_equal('cat ' . $HOME . '/.vimrc', expandcmd('cat $HOME/.vimrc'))

" ── window-view / prompt / server: inactive standalone ──
call assert_equal([0, 0], win_id2tabwin(1))
call assert_equal(-1, win_splitmove(1, 2))
call assert_equal(0, win_move_separator(1, 5))
call assert_equal('', getcmdwintype())
call assert_equal(0, winrestview({}))
call assert_equal(1, winsaveview()['lnum'])
call assert_equal(0, winsaveview()['leftcol'])
call assert_equal('', prompt_getinput(1))
call assert_equal('Cannot open file', swapinfo('/nope')['error'])
call assert_equal('', serverstart())
call assert_equal({}, api_info())

" ── demo ──
echo 'home is' expand('~')

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: expand assertions passed'
