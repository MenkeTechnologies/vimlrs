" fullcommand.vim — fullcommand(), expanding an abbreviated Ex command name to
" its full form (ported from Neovim's ex_docmd.c). The input must be at least
" the command's minimum abbreviation and a prefix of its full name.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- a single-letter abbreviation expands to its command
call assert_equal('edit', fullcommand('e'))
call assert_equal('substitute', fullcommand('s'))
call assert_equal('global', fullcommand('g'))
call assert_equal('write', fullcommand('w'))

" --- longer prefixes select more specific commands
call assert_equal('split', fullcommand('sp'))
call assert_equal('vsplit', fullcommand('vs'))
call assert_equal('echo', fullcommand('ec'))
call assert_equal('normal', fullcommand('norm'))
call assert_equal('bnext', fullcommand('bn'))

" --- the full name maps to itself
call assert_equal('edit', fullcommand('edit'))
call assert_equal('substitute', fullcommand('substitute'))

" --- a leading ':' and a trailing '!' are stripped before matching
call assert_equal('write', fullcommand(':w'))
call assert_equal('quit', fullcommand('q!'))

" --- an unknown command resolves to an empty string
call assert_equal('', fullcommand('zzz'))
call assert_equal('', fullcommand(''))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'fullcommand.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'fullcommand.vim: all assertions passed'
