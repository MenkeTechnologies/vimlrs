" :defer — call a function when the current function is done.
"
" Every expected value below is what vim 9.2 produces for the same script
" (/opt/homebrew/bin/vim, `vim -u NONE -N -es -c 'source …'`), not a reading of
" the docs. The three behaviours that make :defer more than sugar for a call at
" the end of the body are:
"
"   1. arguments are evaluated at the :defer, not when the deferred call runs
"   2. deferred calls run last-registered-first
"   3. they run when the function unwinds through a :throw, before the caller's
"      :catch sees the exception — and the exception still propagates
"
" A plain global is used as the log so ordering is observable without files.

let g:log = []

func! Log(msg)
  call add(g:log, a:msg)
endfunc

" 2: LIFO. Two defers in a function run newest-first, after the body.
func! Lifo()
  defer Log('first-registered')
  defer Log('second-registered')
  call Log('body')
endfunc

let g:log = []
call Lifo()
call assert_equal(['body', 'second-registered', 'first-registered'], g:log,
      \ 'deferred calls run after the body, last-registered-first')

" 1: arguments are captured at the :defer. Reassigning the variable afterwards
" must not change what the deferred call receives.
func! EagerArgs()
  let x = 'at-defer-time'
  defer Log(x)
  let x = 'at-call-time'
  call Log('body-' . x)
endfunc

let g:log = []
call EagerArgs()
call assert_equal(['body-at-call-time', 'at-defer-time'], g:log,
      \ 'a deferred call keeps the argument value from when :defer ran')

" 3: the deferred call runs while unwinding, before the :catch, and the
" exception is not swallowed by it.
func! Thrower()
  defer Log('deferred-on-throw')
  throw 'boom'
endfunc

let g:log = []
try
  call Thrower()
catch
  call Log('caught-' . v:exception)
endtry
call assert_equal(['deferred-on-throw', 'caught-boom'], g:log,
      \ 'a deferred call runs during unwind, before the catch, and the throw still propagates')

" A function that defers nothing is unaffected, and a deferred call in a nested
" function runs at *that* function's exit rather than the caller's.
func! Inner()
  defer Log('inner-defer')
  call Log('inner-body')
endfunc

func! Outer()
  defer Log('outer-defer')
  call Inner()
  call Log('outer-body')
endfunc

let g:log = []
call Outer()
call assert_equal(['inner-body', 'inner-defer', 'outer-body', 'outer-defer'], g:log,
      \ "a nested call's defers run at its own exit, not the caller's")

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'defer.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'defer.vim: all assertions passed'
