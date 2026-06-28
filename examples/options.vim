" options.vim — setting options from expressions (let &opt = …, :setlocal),
" the \= substitute-with-expression, and the buffer query builtins
" matchbufline()/getbufinfo() made real on the in-memory buffer.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- &opt reads an option; let &opt = … and :set/:setlocal write it
call assert_equal(8, &shiftwidth)
let &shiftwidth = 4
call assert_equal(4, &shiftwidth)
setlocal shiftwidth=2
call assert_equal(2, &shiftwidth)
set shiftwidth=8
call assert_equal(8, &shiftwidth)

" --- a boolean option round-trips through let &opt
let &ignorecase = 1
call assert_equal(1, &ignorecase)
let &ignorecase = 0
call assert_equal(0, &ignorecase)

" --- :> shift honors the 'shiftwidth' we just set
call setline(1, ['x'])
set shiftwidth=3
:1>
call assert_equal('   x', getline(1))

" --- \= substitute evaluates the replacement as an expression per match
call assert_equal('a10b20', substitute('a5b10', '\d\+', '\=submatch(0) * 2', 'g'))
call deletebufline('', 1, '$')
call setline(1, ['x3', 'y10', 'z7'])
:%s/\d\+/\=submatch(0) + 1/
call assert_equal(['x4', 'y11', 'z8'], getline(1, '$'))

" --- matchbufline() returns every {pat} match in a line range
call deletebufline('', 1, '$')
call setline(1, ['foo bar', 'baz foo qux', 'no hit'])
call assert_equal(
  \ [{'lnum': 1, 'byteidx': 0, 'text': 'foo'}, {'lnum': 2, 'byteidx': 4, 'text': 'foo'}],
  \ matchbufline(1, 'foo', 1, '$'))

" --- getbufinfo() describes the single virtual buffer
let bi = getbufinfo()[0]
call assert_equal(1, bi.bufnr)
call assert_equal(3, bi.linecount)
call assert_equal(1, bi.loaded)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'options.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'options.vim: all assertions passed'
