" nohlsearch.vim — the :nohlsearch Ex command (ex_docmd.c: ex_nohlsearch).
" `:noh[lsearch]` clears the current search-match highlighting. There is no
" highlight state in an editor-less config load, so it is a no-op — but it MUST
" parse as an Ex command, not fall through to expression evaluation (which would
" raise E121: Undefined variable: nohlsearch). Real Vim 9.2 sources a bare
" `nohlsearch` line silently; runtime syntax files (e.g. syntax/colortest.vim)
" rely on this. All abbreviations from :noh down to :nohlsearch are accepted.
" Self-test: a sentinel is set only if every form is reached without aborting.

let s:reached = 0

" --- every documented abbreviation parses and runs as a no-op
noh
nohl
nohls
nohlse
nohlsea
nohlsear
nohlsearc
nohlsearch
let s:reached += 1

" --- works inside a function body (must not abort the :function)
function! s:ClearHl() abort
  set hlsearch
  nohlsearch
  return 'cleared'
endfunction
call assert_equal('cleared', s:ClearHl())

" --- a bare `noh` at statement start is the command, never a variable read
call assert_equal(1, s:reached)

" --- an ordinary variable whose name merely starts with those letters is
"     unaffected: only a statement-position bare word is the command
let g:nohuh = 42
call assert_equal(42, g:nohuh)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'nohlsearch.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'nohlsearch.vim: all assertions passed'
