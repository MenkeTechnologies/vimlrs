" mappings.vim — a real key-mapping table: mapset() creates mappings and
" maparg()/mapcheck()/maplist()/hasmapto() query them (ported from Neovim's
" mapping.c). A mapping is just data, so this works fully standalone.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- nothing is mapped initially
call assert_equal('', maparg('<C-a>'))
call assert_equal([], maplist())
call assert_equal(0, hasmapto('<Esc>'))

" --- mapset({dict}) creates a mapping; maparg() returns its rhs for the mode
call mapset({'lhs': '<C-a>', 'rhs': ':wall<CR>', 'mode': 'n', 'noremap': 1, 'silent': 1})
call mapset({'lhs': 'jj', 'rhs': '<Esc>', 'mode': 'i'})
call assert_equal(':wall<CR>', maparg('<C-a>', 'n'))
call assert_equal('<Esc>', maparg('jj', 'i'))

" --- mappings are mode-scoped: an insert-mode mapping is invisible in normal
call assert_equal('', maparg('jj', 'n'))

" --- maparg() with a truthy {dict} argument returns the full mapping Dict
let m = maparg('<C-a>', 'n', 0, 1)
call assert_equal('<C-a>', m.lhs)
call assert_equal(':wall<CR>', m.rhs)
call assert_equal('n', m.mode)
call assert_equal(1, m.noremap)
call assert_equal(1, m.silent)
call assert_equal(0, m.expr)

" --- hasmapto() checks the rhs side; mapcheck() prefix-matches the lhs
call assert_equal(1, hasmapto('<Esc>', 'i'))
call assert_equal(0, hasmapto('<Esc>', 'n'))
call assert_equal(':wall<CR>', mapcheck('<C-a>', 'n'))
call assert_equal('', mapcheck('zz', 'n'))

" --- a mapping with the default ('  ') mode is visible in normal mode and
"     reports its mode as a single space
call mapset({'lhs': '<leader>x', 'rhs': ':bd<CR>', 'mode': ' '})
call assert_equal(':bd<CR>', maparg('<leader>x', 'n'))
call assert_equal(' ', maparg('<leader>x', 'n', 0, 1).mode)

" --- the older mapset({mode}, {abbr}, {dict}) form also works
call mapset('n', 0, {'lhs': 'gb', 'rhs': ':bnext<CR>'})
call assert_equal(':bnext<CR>', maparg('gb', 'n'))

" --- maplist() returns every mapping; re-mapping an lhs replaces it in place
call assert_equal(4, len(maplist()))
call mapset({'lhs': '<C-a>', 'rhs': ':qall<CR>', 'mode': 'n'})
call assert_equal(':qall<CR>', maparg('<C-a>', 'n'))
call assert_equal(4, len(maplist()))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'mappings.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'mappings.vim: all assertions passed'
