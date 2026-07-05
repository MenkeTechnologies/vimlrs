" command_abbreviations.vim — every statement below is an Ex command a real
" .vimrc puts inside a :function body. Before these were recognized, an
" unrecognized one fell through to the expression parser, aborted the enclosing
" :function, and leaked its body (`a:`/`l:` refs) into global scope. Each test
" here proves the function DEFINES and its scoped variables resolve — i.e. the
" body was parsed as the function's, not leaked. Self-test: asserts into
" v:errors, throws at the end if anything failed.

" --- :execute accepts every abbreviation down to :exe (exec/execu/execut). The
"     command word before them (`let l:x`) must stay inside the function.
function! F_Exec(n) abort
  let l:x = a:n * 2
  exec 'let l:x = l:x + 1'
  execu 'let l:x = l:x + 10'
  return l:x
endfunction
call assert_equal(1, exists('*F_Exec'))
call assert_equal(15, F_Exec(2))

" --- block-keyword abbreviations: fun/endfun (=:function/:endfunction), wh/endw
"     (=:while/:endwhile). A body defined with them must close correctly.
fun! F_Block(limit)
  let l:sum = 0
  let l:i = 0
  wh l:i < a:limit
    let l:sum += l:i
    let l:i += 1
  endw
  return l:sum
endfun
call assert_equal(1, exists('*F_Block'))
call assert_equal(6, F_Block(4))

" --- :echohl {group} sets a highlight group (no-op editor-less); its argument is
"     a group NAME, not an expression. A body using it must still define.
function! F_Echohl(v) abort
  echohl ErrorMsg
  let l:r = a:v
  echohl None
  return l:r
endfunction
call assert_equal(1, exists('*F_Echohl'))
call assert_equal(7, F_Echohl(7))

" --- :normal, :! (shell), :redraw!, :redir, :mark, and a bare mark-address
"     (`'>`) are recognized editor commands; each is a no-op here but must parse.
function! F_Editor(v) abort
  let l:r = a:v
  normal! gg
  silent !true > /dev/null 2>&1
  redraw!
  mark z
  '>
  return l:r
endfunction
call assert_equal(1, exists('*F_Editor'))
call assert_equal(9, F_Editor(9))

" --- :echoerr (abbrev echoe/echoer) parses like :echomsg. In a never-taken
"     branch it must not abort the definition.
function! F_Echoerr(bad) abort
  let l:r = 3
  if a:bad
    echoerr 'should not run'
  endif
  return l:r
endfunction
call assert_equal(1, exists('*F_Echoerr'))
call assert_equal(3, F_Echoerr(0))

" --- bare :function / :let are listing commands (no parens / no `=`), not a
"     nested definition / a broken assignment. A body listing them still defines.
function! F_Listing(v) abort
  let l:r = a:v
  silent function
  silent let
  return l:r
endfunction
call assert_equal(1, exists('*F_Listing'))
call assert_equal(5, F_Listing(5))

" --- a:000 varargs must resolve inside a `...` function (regression: the body
"     leaked and a:000 escaped to global).
function! F_Varargs(...)
  return len(a:000)
endfunction
call assert_equal(3, F_Varargs('a', 'b', 'c'))
call assert_equal(0, F_Varargs())

" --- :autocmd absorbs a trailing `|` as part of its command; the splitter must
"     keep the line whole (else the `| let` breaks off as a stray statement and,
"     worse, the trailing word leaks). Registering it must not run the command.
let g:ac_ran = 0
autocmd User AbbrevTest call add([], 1) | let g:ac_ran = g:ac_ran
call assert_equal(1, exists('#User#AbbrevTest'))
call assert_equal(0, g:ac_ran)

" --- :source expands $VAR/~ in its path (Vim's do_source), like glob() does.
let $VIMLRS_SRCDIR = tempname()
call mkdir($VIMLRS_SRCDIR, 'p')
call writefile(['let g:sourced_ok = 123'], $VIMLRS_SRCDIR . '/inc.vim')
let g:sourced_ok = 0
execute 'source ' . $VIMLRS_SRCDIR . '/inc.vim'
call assert_equal(123, g:sourced_ok)
" the env var is expanded from an unquoted path too
let g:sourced_ok = 0
source $VIMLRS_SRCDIR/inc.vim
call assert_equal(123, g:sourced_ok)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'command_abbreviations.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'command_abbreviations.vim: all assertions passed'
