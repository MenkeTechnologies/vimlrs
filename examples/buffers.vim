" buffers.vim — buffer/window/tab introspection + UTF-16 indexing, with tests.
"
" A standalone interpreter has no buffers, windows, or tab pages, so the
" buffer/window builtins return the documented \"absent\" values (a missing
" buffer is -1/0/\"\", an unmeasurable window is -1, one implicit window/tab).
" Also shows strutf16len()/utf16idx() (UTF-16 code-unit counting, where an
" astral character such as an emoji is a surrogate pair = 2 units). Self-checks.
"
"   vimlrs examples/buffers.vim

" ── buffer builtins: nothing is loaded ──
call assert_equal(-1, bufnr('%'))
call assert_equal(0, bufexists('foo'))
call assert_equal(0, buflisted('foo'))
call assert_equal(0, bufloaded('foo'))
call assert_equal('', bufname('%'))
call assert_equal(-1, bufwinnr('foo'))
call assert_equal(-1, bufwinid('foo'))

" ── window/tab builtins: one implicit window and tab page ──
call assert_equal(1, winnr())
call assert_equal(1, tabpagenr())
call assert_equal(1, tabpagewinnr(1))
call assert_equal(-1, winbufnr(1))
call assert_equal(-1, winwidth(0))
call assert_equal(-1, winheight(0))
call assert_equal([], winlayout())
call assert_equal('', winrestcmd())

" ── UTF-16 length / index ──
call assert_equal(5, strutf16len('hello'))
" An emoji is one character but two UTF-16 code units (a surrogate pair).
let withemoji = 'a' . nr2char(0x1F600) . 'b'
call assert_equal(4, strutf16len(withemoji))
call assert_equal(3, strchars(withemoji))
" Byte 5 is the 'b' (a=1 byte, emoji=4 bytes); its UTF-16 index is 1 + 2 = 3.
call assert_equal(3, utf16idx(withemoji, 5))
call assert_equal(5, utf16idx('hello', 5))

" ── demo: globpath() collects matches across several directories ──
echo 'example scripts found:' len(split(globpath('examples', '*.vim'), "\n"))

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: buffers assertions passed'
