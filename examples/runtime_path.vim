" runtime_path.vim — `:runtime[!] {file}` sources files from 'runtimepath'.
"
" Vim's `:runtime` (ex_docmd.c → ex_runtime → source_runtime) searches the
" 'runtimepath' option, honoring a live `set runtimepath+=DIR`. This script
" builds a throwaway runtime tree under the current directory, adds it to &rtp,
" and checks that:
"   * before the `set rtp+=`, `:runtime plugin/…` finds nothing (not in &rtp),
"   * after it, `:runtime` sources the first match, and
"   * `:runtime!` with a wildcard sources every match.
" Oracle-verified: real Vim 9.2 produces byte-identical results (errors=0,
" every flag set) for the same script. Self-checks.
"
"   vimlrs examples/runtime_path.vim   (run from the project root)

" ── set up a throwaway runtime tree ──
call mkdir('rtdemo/plugin', 'p')
call mkdir('rtdemo/ftplugin', 'p')
call writefile(['let g:rt_plugin = 1'], 'rtdemo/plugin/rtdemo.vim')
call writefile(['let g:rt_ftp_a = 1'], 'rtdemo/ftplugin/aaa.vim')
call writefile(['let g:rt_ftp_b = 1'], 'rtdemo/ftplugin/aab.vim')
call writefile(['let g:rt_after_remove = 1'], 'rtdemo/plugin/after.vim')

" ── before adding rtdemo to &rtp, :runtime cannot find it ──
runtime plugin/rtdemo.vim
call assert_equal(-1, get(g:, 'rt_plugin', -1))

" ── add the tree to 'runtimepath'; Vim honors this live ──
set runtimepath+=rtdemo

" ── :runtime sources the FIRST match found on &rtp ──
runtime plugin/rtdemo.vim
call assert_equal(1, get(g:, 'rt_plugin', -1))

" ── :runtime! sources EVERY match; the final path component may glob ──
runtime! ftplugin/aa?.vim
call assert_equal(1, get(g:, 'rt_ftp_a', -1))
call assert_equal(1, get(g:, 'rt_ftp_b', -1))

" ── set rtp-=DIR removes the entry: a file only in that tree is unreachable.
"    (Asserting behavior, not the &rtp string — Vim's default rtp is non-empty
"    while vimlrs' stored rtp starts empty, so the string values differ.) ──
set runtimepath-=rtdemo
runtime plugin/after.vim
call assert_equal(-1, get(g:, 'rt_after_remove', -1))

" ── cleanup ──
call delete('rtdemo', 'rf')

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: runtime_path assertions passed'
