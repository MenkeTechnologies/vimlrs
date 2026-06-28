" sfile_helper.vim — sourced by examples/sfile.vim to check that <sfile> inside
" a :source'd file resolves to THIS file, not the parent. Records its own tail.
let g:sfile_helper_seen = expand('<sfile>:t')
