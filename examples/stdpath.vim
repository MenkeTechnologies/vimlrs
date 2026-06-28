" stdpath.vim — XDG standard paths, plus the last editor/provider builtins.
"
" stdpath() resolves Nvim's standard directories from the XDG base-directory
" environment variables (with ~/.config-style defaults) and the 'nvim' app
" subdir. The remaining builtins (keytrans, the lua/ruby providers, terminal,
" GUI file dialogs, 'path' search) have no standalone backing and return their
" documented inactive values. Self-checks.
"
"   vimlrs examples/stdpath.vim

let home = $HOME

" ── stdpath(): XDG dirs + the 'nvim' app subdir ──
call assert_equal(home . '/.cache/nvim', stdpath('cache'))
call assert_equal(home . '/.local/share/nvim', stdpath('data'))
call assert_equal(home . '/.local/state/nvim', stdpath('state'))
call assert_equal(home . '/.local/state/nvim/logs', stdpath('log'))
call assert_equal(home . '/.config/nvim', stdpath('config'))
" The *_dirs kinds return a List.
call assert_equal(type([]), type(stdpath('config_dirs')))
call assert_equal(type(''), type(stdpath('run')))

" ── remaining inactive builtins ──
call assert_equal('plain', keytrans('plain'))
call assert_equal(v:null, luaeval('vim.fn'))
call assert_equal(v:null, rubyeval('1'))
call assert_equal(-1, termopen('sh'))
call assert_equal('', browse(0, 'Save', '.', ''))
call assert_equal('', findfile('nope.xyz'))
call assert_equal('', finddir('nope'))

" ── demo ──
echo 'config dir would be:' stdpath('config')

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: stdpath assertions passed'
