" closures.vim — lambdas close over the enclosing function's scope.
"
" A `{args -> expr}` lambda captures the local scope of the function that
" creates it: the function's arguments (a:) and locals (l:/bare names). The
" values are captured when the lambda is created, so lambdas made in separate
" calls are independent. Dynamic scopes (g:, b:, …) are NOT captured — they
" resolve when the lambda runs. Self-tests into v:errors.
"
"   vimlrs examples/closures.vim

" ── capture a function argument (a:) ──
function! Adder(n) abort
  return {x -> x + a:n}
endfunction
let Add5 = Adder(5)
call assert_equal(15, Add5(10))
call assert_equal(5, Adder(2)(3))

" ── capture a local (l:/bare) ──
function! Multiplier() abort
  let factor = 3
  return {x -> x * factor}
endfunction
call assert_equal(12, Multiplier()(4))

" ── lambdas from separate calls are independent (captured by value) ──
function! Const(v) abort
  return {-> a:v}
endfunction
let one = Const(1)
let two = Const(2)
call assert_equal(1, one())
call assert_equal(2, two())

" ── a captured closure works inside map()/filter() ──
function! ScaleAll(list, by) abort
  return map(copy(a:list), {i, v -> v * a:by})
endfunction
call assert_equal([10, 20, 30], ScaleAll([1, 2, 3], 10))

function! AtLeast(list, min) abort
  return filter(copy(a:list), {i, v -> v >= a:min})
endfunction
call assert_equal([5, 8], AtLeast([2, 5, 1, 8], 5))

" ── nested lambdas capture through each layer ──
function! Curry(a) abort
  return {b -> {c -> a:a + b + c}}
endfunction
call assert_equal(123, Curry(100)(20)(3))

" ── a global is read when the lambda runs, not captured ──
let g:counter = 10
function! ReadCounter() abort
  return {-> g:counter}
endfunction
let GetIt = ReadCounter()
let g:counter = 99
call assert_equal(99, GetIt())

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: closures assertions passed'
