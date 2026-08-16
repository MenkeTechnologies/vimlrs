" prepare_assert_error() — the location stamp every v:errors entry opens with.
"
" A failure recorded by any assert_*() is prefixed with the exestack chain and
" the line it was raised on: `<script> line N: <message>`. Before this was
" ported, v:errors held the bare message and every failure looked like it came
" from nowhere.
"
" The raw value embeds this file's absolute path, so — exactly as
" `throwpoint.vim` does for v:throwpoint — everything up to and including the
" last path separator is dropped. That also drops the one entry this interpreter
" structurally cannot have: vim was launched with `-c 'source …'` so its chain
" opens `command line..script …`, while vimlrs is handed the script path
" directly. Everything from the basename on is compared verbatim, which is where
" the whole prefix (frame chain, per-frame `[N]` call sites, `line N: `) lives.
function! Tail()
  let e = v:errors[0]
  let v:errors = []
  return substitute(e, '.*[/\\]', '', '')
endfunction

call assert_equal(1, 2)
echo Tail()

" The line number is the raising line, not the enclosing function's first line.
call assert_true(0)
echo Tail()

" A user {msg} goes AFTER the location stamp, not before it.
call assert_equal(1, 2, 'custom')
echo Tail()

" assert_report() is stamped too — it has no comparison, only the message.
call assert_report('boom')
echo Tail()

" So is assert_inrange(), which builds its message without fill_assert_error().
call assert_inrange(1, 3, 5)
echo Tail()

" And assert_exception(), on the no-exception-pending path.
call assert_exception('nope')
echo Tail()

" Inside a call chain the stamp names every frame, each with the line IT was at
" when it entered the next, and only the innermost gets the `line N` suffix.
function! Inner()
  call assert_equal('a', 'b')
endfunction
function! Outer()
  call Inner()
endfunction
call Outer()
echo Tail()

" A failing assert still returns 1 / a passing one 0, and neither is disturbed
" by the stamp.
echo assert_equal(1, 1) assert_equal(1, 2)
let v:errors = []
