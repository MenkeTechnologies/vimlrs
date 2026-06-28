" sfile.vim — expand('<sfile>') resolves to the sourced script's own path
" (funcs.c f_expand + the sourcing-name stack). '<sfile>' and '<script>' expand
" to the path of the script currently being sourced, and accept the same trailing
" ':' filename-modifiers as fnamemodify(). Self-test into v:errors.

" --- <sfile> is this file; its tail is 'sfile.vim' and extension 'vim'
call assert_match('sfile\.vim$', expand('<sfile>'))
call assert_equal('sfile.vim', expand('<sfile>:t'))
call assert_equal('vim', expand('<sfile>:e'))
call assert_equal('sfile', expand('<sfile>:t:r'))

" --- :p:h gives the containing directory (an absolute path, no trailing name)
call assert_match('examples$', expand('<sfile>:p:h'))
call assert_equal(0, expand('<sfile>:p:h') =~ 'sfile\.vim')

" --- <script> resolves the same way as <sfile> for a sourced file
call assert_equal(expand('<sfile>'), expand('<script>'))
call assert_equal(expand('<sfile>:t'), expand('<script>:t'))

" --- expand() of a $VAR / wildcard path still works (no special token)
call assert_equal($HOME, expand('$HOME'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'sfile.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'sfile.vim: all assertions passed'
