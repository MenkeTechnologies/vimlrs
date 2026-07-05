" deferred_function.vim — a `:function` nested inside a control-flow block
" (`:if`/`:while`/`:for`/`:try`) or another function body is registered when its
" line EXECUTES, not at parse time (userfunc.c:2485-2511: those blocks only
" adjust `indent`, and an inner `:function …(` is defined when the enclosing code
" runs). This is the idempotent `if !exists('*F') | function F() … | endif`
" idiom that real `.vimrc`s and colour schemes (e.g. zpwrmarklar.vim) rely on.
" Verified against Vim 9.2. Self-test: asserts into v:errors, throws if any fail.

" --- 1. a function defined inside a TRUE :if block is callable afterward
if 1
  function! TrueGuard()
    return 42
  endfunction
endif
call assert_equal(1, exists('*TrueGuard'))
call assert_equal(42, TrueGuard())

" --- 2. a function inside a FALSE :if guard is never defined
if 0
  function! FalseGuard()
    return 7
  endfunction
endif
call assert_equal(0, exists('*FalseGuard'))

" --- 3. the `if !exists('*F')` idiom is idempotent: a second guard block at
"        script level (as a re-source would hit) sees the function already
"        defined and does NOT redefine it — the original body wins.
if !exists('*ScriptGuard')
  function! ScriptGuard()
    return 'first'
  endfunction
endif
if !exists('*ScriptGuard')
  function! ScriptGuard()
    return 'second'
  endfunction
endif
call assert_equal('first', ScriptGuard())

" --- 4. a function defined inside ANOTHER function's body (def-in-def) is
"        registered when the OUTER function runs — not at parse time, and NOT an
"        error (Vim's E120 is unrelated: "Using <SID> not in a script context").
function! Outer()
  function! Inner()
    return 'inner'
  endfunction
endfunction
call assert_equal(0, exists('*Inner'))
call Outer()
call assert_equal(1, exists('*Inner'))
call assert_equal('inner', Inner())

" --- 5. the guarded idiom inside an init function is also idempotent across
"        repeated calls (the common autoload/plugin-init shape).
function! s:init() abort
  if !exists('*InitFn')
    function! InitFn()
      return 'v1'
    endfunction
  endif
endfunction
call s:init()
call assert_equal('v1', InitFn())
call s:init()
call assert_equal('v1', InitFn())

" --- 6. deferred inside a :while loop: defined on the first reached iteration
let s:n = 0
while s:n < 3
  if !exists('*LoopFn')
    function! LoopFn()
      return 'looped'
    endfunction
  endif
  let s:n += 1
endwhile
call assert_equal('looped', LoopFn())

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'deferred_function.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'deferred_function.vim: all assertions passed'
