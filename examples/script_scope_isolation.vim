" script_scope_isolation.vim — each sourced script gets its own `s:` scope.
"
" Vim gives every sourced file a private script-local (`s:`) scope keyed by its
" script id, so a nested `:source`/`:runtime` file's `s:` variables neither leak
" into nor clobber the sourcing script's. This is what lets the runtime ftplugin
" idiom work: a parent sets `s:save_cpo = &cpo`, `runtime!`s sibling ftplugins
" that each set AND `unlet` their own `s:save_cpo`, then restores `&cpo` from its
" own still-intact `s:save_cpo`. If the scope were shared, the child's
" `unlet s:save_cpo` would delete the parent's and the restore would raise
" `E121: Undefined variable: s:save_cpo`. Self-checks; exits non-zero on failure.
"
"   vimlrs examples/script_scope_isolation.vim   (run from the project root)

" ── the parent's own script scope ──
let s:save_cpo = 'parent'
let s:parent_only = 42

" Source a child that sets, reads and unlets its own `s:save_cpo`.
source examples/lib/scope_child.vim

" ── the child saw ITS values, never the parent's ──
call assert_equal('child', g:child_saw_save_cpo)
call assert_true(g:child_saw_own)
" ...and its own `unlet` removed its own copy (within its scope).
call assert_false(g:child_after_unlet)

" ── the parent's scope is untouched by the child's set/unlet ──
call assert_equal('parent', s:save_cpo)
call assert_equal(42, s:parent_only)
" The child's private var never leaked into the parent scope.
call assert_false(exists('s:child_only'))

" ── the parent can now run its own cpo-restore epilogue without E121 ──
let g:restored = s:save_cpo
unlet s:save_cpo
call assert_equal('parent', g:restored)
call assert_false(exists('s:save_cpo'))

" ── re-sourcing the child again still isolates cleanly ──
let s:save_cpo = 'parent2'
source examples/lib/scope_child.vim
call assert_equal('child', g:child_saw_save_cpo)
call assert_equal('parent2', s:save_cpo)

" ── demo ──
echo 'parent kept s:save_cpo =' 'parent2' '/ child saw' g:child_saw_save_cpo

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: script_scope_isolation assertions passed'
