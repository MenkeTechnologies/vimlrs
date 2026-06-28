" editor_compat.vim — editor-position builtins under the standalone runtime,
" with embedded unit tests pinning the documented \"no editor\" return values.
"
" Vimscript written for Vim/Neovim often calls cursor/screen/search builtins.
" A standalone interpreter has no buffer, window, or screen grid, so these
" return the same \"nothing here\" values the editor returns when the subsystem
" is inactive — letting editor-oriented scripts load and run instead of
" erroring. The asserts lock those contracts so a regression fails CI.
"
"   vimlrs examples/editor_compat.vim

" ── unit tests: the standalone-absent contracts ──
call assert_equal([0, 0, 0, 0], getpos('.'))
call assert_equal([0, 0, 0, 0, 0], getcurpos())
call assert_equal(0, line('.'))
call assert_equal(0, col('.'))
call assert_equal(0, virtcol('.'))
call assert_equal(0, search('foo'))
call assert_equal([0, 0], searchpos('foo'))
call assert_equal(-1, screenchar(1, 1))
call assert_equal([], synstack(1, 1))
call assert_equal(0, synID(1, 1, 1))
call assert_equal(['', ''], spellbadword('teh'))
call assert_equal({'bytes': 0, 'chars': 0, 'words': 0, 'cursor_bytes': 0, 'cursor_chars': 0, 'cursor_words': 0}, wordcount())
call assert_equal({'char': '', 'forward': 1, 'until': 0}, getcharsearch())

" Setters report failure (no buffer/window to act on), not a crash.
call assert_equal(-1, setpos('.', [0, 1, 1, 0]))
call assert_equal(-1, cursor(1, 1))

" ── demo ──
echo 'getpos(.)   :' getpos('.')
echo 'search(foo) :' search('foo')
echo 'wordcount() :' wordcount()

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: editor_compat assertions passed'
