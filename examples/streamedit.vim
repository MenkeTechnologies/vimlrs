" streamedit.vim — vimlrs as a stream editor: file I/O (:e/:r/:w), :sort, the
" :>/:< shift, and the :!/:%! shell filter (ported from Neovim's ex_cmds.c /
" ops.c). These turn the in-memory buffer into a sed/awk-style pipeline.
" Self-test: asserts into v:errors, throws at the end if anything failed.

let s:in = '/tmp/vimlrs_se_in.txt'
let s:out = '/tmp/vimlrs_se_out.txt'

" --- :edit loads a file into the buffer
call writefile(['cherry', 'apple', 'banana'], s:in)
exe ':edit ' . s:in
call assert_equal(['cherry', 'apple', 'banana'], getline(1, '$'))

" --- :sort sorts the whole buffer; :sort! reverses
:sort
call assert_equal(['apple', 'banana', 'cherry'], getline(1, '$'))
:sort!
call assert_equal(['cherry', 'banana', 'apple'], getline(1, '$'))

" --- :sort n sorts numerically; :sort u removes duplicates
call deletebufline('', 1, '$')
call setline(1, ['30', '4', '100', '20'])
:sort n
call assert_equal(['4', '20', '30', '100'], getline(1, '$'))
call deletebufline('', 1, '$')
call setline(1, ['a', 'a', 'b', 'a', 'b'])
:sort u
call assert_equal(['a', 'b'], getline(1, '$'))

" --- :w writes the buffer to a file (:r reads it back, after the range line)
call deletebufline('', 1, '$')
call setline(1, ['line one', 'line two'])
exe ':w ' . s:out
call assert_equal(['line one', 'line two'], readfile(s:out))
exe ':$r ' . s:out
call assert_equal(['line one', 'line two', 'line one', 'line two'], getline(1, '$'))

" --- :> indents by 'shiftwidth', :< dedents; blank lines are left alone
call deletebufline('', 1, '$')
call setline(1, ['x', '', 'y'])
set shiftwidth=4
:1,3>
call assert_equal(['    x', '', '    y'], getline(1, '$'))
:1,3<
call assert_equal(['x', '', 'y'], getline(1, '$'))

" --- :%!cmd filters the buffer through a shell command
call deletebufline('', 1, '$')
call setline(1, ['banana', 'apple', 'cherry'])
:%!sort
call assert_equal(['apple', 'banana', 'cherry'], getline(1, '$'))

" --- :r !cmd inserts a shell command's output after the range line
call deletebufline('', 1, '$')
call setline(1, ['top'])
:r !printf 'a\nb\n'
call assert_equal(['top', 'a', 'b'], getline(1, '$'))

call delete(s:in)
call delete(s:out)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'streamedit.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'streamedit.vim: all assertions passed'
