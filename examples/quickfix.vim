" quickfix.vim — getqflist()/setqflist()/getloclist()/setloclist(), the
" quickfix and location error lists ported from Neovim's quickfix.c. Standalone
" they are real in-memory error lists (no window to display them in).
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- a fresh quickfix list is empty
call assert_equal([], getqflist())

" --- setqflist() normalizes each entry to the full quickfix schema
call assert_equal(0, setqflist([{'lnum': 10, 'text': 'oops', 'bufnr': 1}, {'text': 'no line'}]))
let qf = getqflist()
call assert_equal(2, len(qf))
call assert_equal(10, qf[0].lnum)
call assert_equal('oops', qf[0].text)
call assert_equal(1, qf[0].bufnr)
" an entry with a real buffer/line is 'valid'; a bare-text entry is not
call assert_equal(1, qf[0].valid)
call assert_equal(0, qf[1].valid)
" the schema always carries these keys
call assert_equal(0, qf[0].end_lnum)
call assert_equal('', qf[0].type)

" --- getqflist({what}) returns the requested properties as a Dict
call assert_equal({'size': 2, 'title': ''}, getqflist({'size': 1, 'title': 1}))

" --- action 'a' appends, ' '/'r' replace, 'f' frees the list
call assert_equal(0, setqflist([{'text': 'three'}], 'a'))
call assert_equal(3, len(getqflist()))
call assert_equal(0, setqflist([], 'f'))
call assert_equal([], getqflist())

" --- a {what} with a title is stored and read back
call assert_equal(0, setqflist([{'text': 'x'}], 'r', {'title': 'My Errors'}))
call assert_equal('My Errors', getqflist({'title': 1}).title)

" --- the location list is independent of the quickfix list
call assert_equal(0, setloclist(0, [{'lnum': 5, 'text': 'loc'}]))
call assert_equal(1, len(getloclist(0)))
call assert_equal('loc', getloclist(0)[0].text)
" the quickfix list is unchanged by setloclist()
call assert_equal(1, len(getqflist()))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'quickfix.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'quickfix.vim: all assertions passed'
