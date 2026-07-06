" scope_child.vim — a nested-source helper for script_scope_isolation.vim.
"
" Sourced by that example to prove Vim's per-script `s:` scope: this file sets,
" reads and then `unlet`s its OWN `s:save_cpo` (the cpo save/restore epilogue
" every runtime ftplugin uses). None of that must touch the sourcing script's
" identically named `s:save_cpo`. A global records what this file observed so
" the parent can assert the two scopes never crossed.

let s:save_cpo = 'child'
let s:child_only = 'private'

" This file sees ITS value, not the parent's.
let g:child_saw_save_cpo = s:save_cpo
let g:child_saw_own = exists('s:child_only')

" The ftplugin epilogue: restore and delete our own copy. In a shared scope
" this deletion is what wiped the parent's `s:save_cpo` (→ E121 on its restore).
unlet s:save_cpo
let g:child_after_unlet = exists('s:save_cpo')
