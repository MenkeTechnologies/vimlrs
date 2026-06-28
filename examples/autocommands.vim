" autocommands.vim — :autocmd registers event handlers, :doautocmd fires them,
" :augroup groups them, exists('#…') queries them (ported from Neovim's
" autocmd.c). Events do not auto-fire without an editor, but :doautocmd triggers
" them, so the whole subsystem is testable standalone.
" Self-test: asserts into v:errors, throws at the end if anything failed.

let g:log = []

" --- multiple handlers for an event fire in registration order
autocmd User Foo call add(g:log, 'foo1')
autocmd User Foo call add(g:log, 'foo2')
autocmd User Bar call add(g:log, 'bar')

" --- exists('#event') / exists('#event#pat') report registered autocommands
call assert_equal(1, exists('#User'))
call assert_equal(1, exists('#User#Foo'))
call assert_equal(0, exists('#User#Nope'))
call assert_equal(0, exists('#Other'))

" --- :doautocmd fires every matching handler; a different pattern is untouched
doautocmd User Foo
call assert_equal(['foo1', 'foo2'], g:log)
doautocmd User Bar
call assert_equal(['foo1', 'foo2', 'bar'], g:log)

" --- file-glob patterns match the :doautocmd target
let g:files = []
autocmd BufRead *.txt call add(g:files, 'txt')
autocmd BufRead *.md  call add(g:files, 'md')
doautocmd BufRead notes.txt
doautocmd BufRead readme.md
doautocmd BufRead script.vim
call assert_equal(['txt', 'md'], g:files)

" --- comma-separated events register one handler each
let g:multi = 0
autocmd BufNewFile,BufRead *.log let g:multi += 1
doautocmd BufNewFile app.log
doautocmd BufRead app.log
call assert_equal(2, g:multi)

" --- :augroup sets the group an autocommand belongs to; it still fires
augroup MyGroup
  autocmd User Grouped call add(g:log, 'grouped')
augroup END
doautocmd User Grouped
call assert_equal('grouped', g:log[-1])

" --- :autocmd! {event} {pat} removes handlers (exists then reports 0)
autocmd! User Foo
call assert_equal(0, exists('#User#Foo'))
let g:log = []
doautocmd User Foo
call assert_equal([], g:log)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'autocommands.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'autocommands.vim: all assertions passed'
