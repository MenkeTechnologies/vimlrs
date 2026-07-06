" scriptlocal_funcname.vim — `<SID>`/`<SNR>` script-local function names. Vim
" accepts the `<SID>` marker wherever a function name is expected: a `:function`
" definition, a `:call`, a call inside an expression, `exists('*…')`, and a
" `funcref`. The marker is case-insensitive. Standalone runtime files (e.g.
" ftplugins) define helpers as `func <SID>Foo()` and invoke them with
" `call <SID>Foo()`, so the lexer must scan `<SID>Foo` as one function name and
" not as a `<` comparison. Self-test: asserts into v:errors, throws at the end.

" --- a `<SID>` helper is callable via `:call` and returns its value
func <SID>Double(n)
  return a:n * 2
endfunc
let s:r = 0
call <SID>Double(21)
call assert_equal(42, <SID>Double(21))

" --- callable inside an arbitrary expression, not only via `:call`
call assert_equal(50, <SID>Double(20) + 10)

" --- the argument scope (`a:`) binds normally inside a `<SID>` function
func <SID>Greet(who)
  return 'hi ' .. a:who
endfunc
call assert_equal('hi vim', <SID>Greet('vim'))

" --- exists('*<SID>Foo') reports the script-local function as defined
call assert_equal(1, exists('*<SID>Double'))
call assert_equal(0, exists('*<SID>NoSuchFunc'))

" --- the `<SID>` marker is case-insensitive (matched via STRNICMP in Vim)
call assert_equal(6, <sid>Double(3))

" --- a `<SID>` function can call another `<SID>` function
func <SID>Quad(n)
  return <SID>Double(<SID>Double(a:n))
endfunc
call assert_equal(20, <SID>Quad(5))

" --- store a `<SID>` funcref and call through it
let s:F = function('<SID>Double')
call assert_equal(14, s:F(7))

if !empty(v:errors)
  for s:e in v:errors
    echoerr s:e
  endfor
  throw 'scriptlocal_funcname.vim failed'
endif
