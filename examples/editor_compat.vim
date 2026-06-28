" editor_compat.vim — editor-position builtins under the standalone runtime.
"
" Vimscript written for Vim/Neovim often calls cursor/screen/search builtins.
" A standalone interpreter has no buffer, window, or screen grid, so these
" return the same "nothing here" values the editor itself returns when the
" subsystem is inactive — letting editor-oriented scripts load and run instead
" of erroring. The list/dict shapes stay faithful, so indexing still works.
"
" (Note: :echo treats a trailing \" as the start of a string, so these scripts
" keep comments on their own lines rather than after an :echo.)
"
"   vimlrs examples/editor_compat.vim

" getpos returns [bufnum, lnum, col, off]; getcurpos adds curswant.
echo 'getpos(.)      :' getpos('.')
echo 'getcurpos()    :' getcurpos()
echo 'line/col/vcol  :' line('.') col('.') virtcol('.')
" search returns 0 when not found; searchpos returns [0, 0].
echo 'search(foo)    :' search('foo')
echo 'searchpos(foo) :' searchpos('foo')
echo 'wordcount()    :' wordcount()
echo 'getcharsearch():' getcharsearch()
" screenchar is -1 off-grid (there is no screen grid).
echo 'screenchar 1,1 :' screenchar(1, 1)

" Guard pattern: code that only acts on a real match still works.
let pos = getpos('.')
if pos[1] == 0
  echo 'no cursor in standalone mode — skipping editor action'
endif
