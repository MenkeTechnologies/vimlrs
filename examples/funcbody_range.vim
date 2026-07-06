" funcbody_range.vim — a line-range Ex command inside a :function body must not
" abort the function DEFINITION. Vim's do_one_cmd (ex_docmd.c) reads a leading
" number as the range's first address, so a body line like `1,1fold`, `1,1print`
" or a bare `5` is an Ex command — never an expression statement (a bare number
" line moves the cursor, `:h {address}`). Before the fix vimlrs routed such a
" digit-leading body line to expression parsing, which choked on the trailing
" command word and silently aborted the whole `:function`, leaving the function
" UNDEFINED (E117: Unknown function on the later `:call`). The assertions check
" each function was defined AND (where the command is a clean no-op in both vim
" and vimlrs) reached its `return`. The runtime effect of `:print`/`:{addr}`
" inside a headless function is editor-state (cursor move / screen print) and is
" out of scope — vim aborts those to -1 with no v:errors, so we only assert that
" those functions were DEFINED, which is what the bug broke.

" --- a ranged :fold in the body is a clean no-op in both engines
function! RangeFold() abort
  set foldmethod=manual
  1,1fold
  return 42
endfunction
call assert_equal(42, RangeFold())

" --- a ranged :print body line: definition must survive it
function! RangePrint()
  1,1print
  return 'defined'
endfunction
call assert_true(exists('*RangePrint'))

" --- a two-address :substitute body line (`/e` = no error if no match)
function! RangeSub() abort
  1,1s/\vZZZ/z/e
  return 'subbed'
endfunction
call assert_equal('subbed', RangeSub())

" --- a bare line address (`:1`) body line: definition must survive it
function! BareAddr()
  1
  return 'defined'
endfunction
call assert_true(exists('*BareAddr'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'funcbody_range.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'funcbody_range.vim: all assertions passed'
