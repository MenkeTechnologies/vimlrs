" execute() runs its argument through do_cmdline_cmd(): a conditional left
" open at the end of the command line is closed silently (no E171/E170/E600).
" An open :if/:try just ends; an open :while/:for runs its body once, because
" nothing reaches the :endwhile/:endfor that would loop back.
echo execute('if 1 | echo "if-open"')
echo execute('if 0 | echo "no" | elseif 1 | echo "elseif-open"')
echo execute('if 0 | echo "no" | else | echo "else-open"')
echo execute('try | echo "try-open"')
echo execute('try | echo "catch-open" | catch | echo "never"')
let g:i = 0
echo execute('while g:i < 3 | let g:i += 1 | echo "while" g:i')
echo g:i
echo execute('for x in [10, 20, 30] | echo "for" x')
let g:n = 0
echo execute('for x in [] | let g:n += 1')
echo execute('while 0 | let g:n += 1')
echo g:n
" assert_fails() also runs a command line: an open :if is not a failure.
call assert_fails('if 1', 'E171')
echo len(v:errors)
" A List is ONE command sequence (get_list_line): a block opened on one item
" is closed by a later one.
echo execute(['let g:x = 0', 'if g:x', 'echo "yes"', 'else', 'echo "no"', 'endif'])
echo execute(['let g:j = 0', 'while g:j < 3', 'let g:j += 1', 'echo g:j', 'endwhile'])
echo execute(['for k in [1, 2]', 'echo k * 10', 'endfor', 'echo "done"'])
echo execute(['function! g:ExecListF()', 'return 42', 'endfunction'])
echo g:ExecListF()
