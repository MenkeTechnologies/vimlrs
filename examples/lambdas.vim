" lambdas.vim — {args -> body} lambda expressions, with embedded unit tests.
"
" Lambdas are now lexed/parsed natively and desugar to anonymous functions, so
" map()/filter()/sort()/reduce() can take an inline `{i, v -> …}` instead of a
" string expression or a named function. Self-checks and exits non-zero on
" failure.
"
"   vimlrs examples/lambdas.vim

" ── map / filter ──
call assert_equal([1, 4, 9, 16], map([1, 2, 3, 4], {i, v -> v * v}))
call assert_equal([2, 4, 6], filter([1, 2, 3, 4, 5, 6], {i, v -> v % 2 == 0}))
" The first lambda arg is the index/key.
call assert_equal(['0:a', '1:b'], map(['a', 'b'], {i, v -> i . ':' . v}))

" ── sort with a comparator lambda (ascending, descending, by string) ──
call assert_equal([1, 2, 3], sort([3, 1, 2], {a, b -> a - b}))
call assert_equal([3, 2, 1], sort([3, 1, 2], {a, b -> b - a}))
call assert_equal(['apple', 'banana'], sort(['banana', 'apple'], {a, b -> a > b ? 1 : -1}))

" ── reduce ──
call assert_equal(10, reduce([1, 2, 3, 4], {acc, v -> acc + v}, 0))
call assert_equal(24, reduce([1, 2, 3, 4], {acc, v -> acc * v}, 1))

" ── a lambda stored in a variable: call it directly or via call() ──
let Add = {x, y -> x + y}
call assert_equal(7, Add(3, 4))
call assert_equal(7, call(Add, [3, 4]))

" ── no-argument lambda ──
call assert_equal(42, call({-> 42}, []))

" ── closures: a lambda captures enclosing-scope variables (by value) ──
let n = 10
call assert_equal([11, 12, 13], map([1, 2, 3], {i, v -> v + n}))
let factor = 3
let Mul = {x -> x * factor}
call assert_equal(21, Mul(7))
" Nested closures: the inner lambda captures the outer lambda's parameter.
call assert_equal([[11, 21], [12, 22]], map([1, 2], {i, v -> map([10, 20], {j, w -> w + v})}))

" ── demo ──
echo 'squares :' map(range(1, 5), {i, v -> v * v})
echo 'sum     :' reduce(range(1, 100), {a, v -> a + v}, 0)

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: lambdas assertions passed'
