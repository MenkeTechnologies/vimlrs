" typename() of a lambda, across every combination of DECLARED parameters,
" CAPTURED variables, and Partial-BOUND arguments (BUGS.md R22-O1).
"
" A lambda stores nothing per-function beyond its arity, so vim renders it from
" the declared parameter count `d` and the bound count `k`: `d == 0` or `k > d`
" prints `...`, otherwise `d - k` `any`s.
"
" The subtlety this case exists for: vim keeps a closure's captured variables in
" the funccal chain and OUT of uf_args, so `{-> a}` is a 0-parameter lambda and
" prints `func(...)`. This port desugars each capture into a leading parameter
" pre-bound by a Partial, which without a recorded capture count makes `{-> a}`
" numerically identical to `function({x -> x}, [1])` — and vim prints
" `func(): [unknown]` for that one. `ufunc_T.uf_captures` is that count; it is
" subtracted from BOTH `d` and `k`, since the desugaring inflates both.
"
" Every row below was read out of vim 9.2. The `c*` lambdas capture, the `n*`
" ones do not, and the `p(...)` rows bind an argument on top.
function! Mk()
  let a = 1
  let b = 2
  let r = {}
  let r.c0 = {-> a}
  let r.c0b = {-> a + b}
  let r.c1 = {x -> x + a}
  let r.c2 = {x, y -> x + y + a}
  let r.n0 = {-> 1}
  let r.n1 = {x -> x}
  let r.n2 = {x, y -> x}
  return r
endfunction
let R = Mk()

" declared 0, captured 1/2 — `d == 0`, so `...` despite the bound captures.
echo typename(R.c0)
echo typename(R.c0b)
" declared 1/2, captured 1 — the captures must not be counted as parameters.
echo typename(R.c1)
echo typename(R.c2)
" declared 0/1/2, captured 0 — the baseline shapes.
echo typename(R.n0)
echo typename(R.n1)
echo typename(R.n2)
" One argument bound on top of each: `d - k` drops by one, and a capturing
" lambda must answer the same as its non-capturing counterpart.
echo typename(function(R.c1, [1]))
echo typename(function(R.n1, [1]))
echo typename(function(R.c2, [1]))
echo typename(function(R.n2, [1]))
" Binding onto a 0-parameter capturing lambda stays `...`.
echo typename(function(R.c0, [1]))
" The capture count survives a lambda called through its own Partial.
echo R.c1(10)
echo R.c0()
