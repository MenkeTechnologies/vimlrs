" default_args.vim — optional function arguments (`func F(a, b = expr)`).
"
" A parameter written `name = default` is optional: when the caller omits it,
" the default expression is evaluated at call time (in the argument scope built
" so far, so a later default may reference an earlier argument). Mirrors Vim's
" |optional-function-argument|. Self-tests into v:errors.
"
"   vimlrs examples/default_args.vim

" ── a constant default fills in when the argument is omitted ──
function! Greet(name, greeting = 'Hello') abort
  return a:greeting . ', ' . a:name . '!'
endfunction
call assert_equal('Hello, Ada!', Greet('Ada'))
call assert_equal('Hi, Ada!', Greet('Ada', 'Hi'))

" ── several optional parameters, filled left to right ──
function! Box(w, h = 1, d = 1) abort
  return a:w * a:h * a:d
endfunction
call assert_equal(2, Box(2))
call assert_equal(6, Box(2, 3))
call assert_equal(24, Box(2, 3, 4))

" ── a default may reference an earlier argument ──
function! Span(lo, hi = a:lo + 10) abort
  return [a:lo, a:hi]
endfunction
call assert_equal([5, 15], Span(5))
call assert_equal([5, 9], Span(5, 9))

" ── defaults can be any expression: List/Dict literals, calls, ternaries ──
function! Tags(extra = ['base']) abort
  return a:extra
endfunction
call assert_equal(['base'], Tags())
call assert_equal(['x', 'y'], Tags(['x', 'y']))

function! Mag(n, m = abs(a:n)) abort
  return a:m
endfunction
call assert_equal(7, Mag(-7))

function! Label(n, s = a:n == 1 ? 'one' : 'many') abort
  return a:s
endfunction
call assert_equal('one', Label(1))
call assert_equal('many', Label(4))

" ── optional params combine with `...` varargs ──
function! Acc(base, step = 1, ...) abort
  let total = a:base + a:step
  for x in a:000
    let total += x
  endfor
  return total
endfunction
call assert_equal(1, Acc(0))
call assert_equal(5, Acc(0, 5))
call assert_equal(15, Acc(0, 5, 10))

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: default_args assertions passed'
