" map_commands.vim — the :map-family Ex commands (:nmap/:inoremap/:vunmap/
" :mapclear/…), ported from Neovim's mapping.c. They define real mappings that
" maparg()/maplist() then observe. A mapping is just data, so this runs fully
" standalone.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" The mappings already in place before this script defines any — the baseline
" every count below is relative to.
let s:before = len(maplist())

" --- :nmap / :inoremap define mode-scoped mappings
nmap <C-a> :wall<CR>
inoremap jj <Esc>
call assert_equal(':wall<CR>', maparg('<C-a>', 'n'))
call assert_equal('<Esc>', maparg('jj', 'i'))

" --- :noremap variants set the 'noremap' flag; plain :map leaves it 0
nnoremap <leader>w :w<CR>
nmap <leader>q :q<CR>
call assert_equal(1, maparg('<leader>w', 'n', 0, 1).noremap)
call assert_equal(0, maparg('<leader>q', 'n', 0, 1).noremap)

" --- the <silent>/<expr>/<nowait> argument prefixes set their flags
nnoremap <silent> <leader>s :nohlsearch<CR>
let m = maparg('<leader>s', 'n', 0, 1)
call assert_equal(1, m.silent)
call assert_equal(':nohlsearch<CR>', m.rhs)

" --- :vmap maps in visual+select; the mode char is reported as 'v'
vmap > >gv
call assert_equal('v', maparg('>', 'v', 0, 1).mode)

" --- :unmap removes a single mapping; :mapclear clears a whole mode
" Counted as a DELTA against the mappings that were already live, because an
" engine started with its own defaults loaded has its own baseline (vim and
" nvim each ship a different set). This script adds six: four normal-mode, one
" insert, one visual — `vmap` maps visual+select, and maplist() reports that
" pair as ONE entry with mode 'v'.
call assert_equal(s:before + 6, len(maplist()))
nunmap <C-a>
call assert_equal('', maparg('<C-a>', 'n'))
call assert_equal(s:before + 5, len(maplist()))

imapclear
call assert_equal('', maparg('jj', 'i'))

" --- the map()/filter() builtins are unaffected by the :map command parsing
call assert_equal([2, 4, 6], map([1, 2, 3], 'v:val * 2'))
call assert_equal([2, 4], filter([1, 2, 3, 4], 'v:val % 2 == 0'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'map_commands.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'map_commands.vim: all assertions passed'
