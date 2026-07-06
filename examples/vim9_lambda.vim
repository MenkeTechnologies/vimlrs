vim9script
# vim9_lambda.vim — vim9 arrow lambdas `(params) => body`, with embedded unit
# tests. Distinct from the legacy `{args -> body}` form (see lambdas.vim): the
# vim9 form uses `(a, b) => expr`, accepts `: type` annotations on parameters and
# a return type, and appears in method-call chains (`list->filter((_, v) => …)`).
#
# Everything below is real vim9script, binary-verified against Vim 9.2. Parameter
# and return types are parsed and discarded; only the names bind. Self-tests into
# v:errors.
#
#   vimlrs examples/vim9_lambda.vim

# ── arrow lambda in map / filter method chains ──
assert_equal([1, 4, 9], [1, 2, 3]->mapnew((_, v) => v * v))
assert_equal([2, 4], [1, 2, 3, 4]->filter((_, v) => v % 2 == 0))
# The first parameter is the index/key; `_` is the conventional ignore name.
assert_equal(['0:a', '1:b'], ['a', 'b']->mapnew((i, v) => i .. ':' .. v))

# ── sort / reduce with an arrow comparator / accumulator ──
assert_equal([1, 2, 3], [3, 1, 2]->sort((a, b) => a - b))
assert_equal([3, 2, 1], [3, 1, 2]->sort((a, b) => b - a))
assert_equal(10, [1, 2, 3, 4]->reduce((acc, v) => acc + v, 0))
assert_equal(24, [1, 2, 3, 4]->reduce((acc, v) => acc * v, 1))

# ── chained method calls: lambda result feeds the next `->` ──
assert_equal('1 2 3', [1, 2, 3]->mapnew((_, v) => string(v))->join(' '))

# ── parameter and return type annotations are accepted and ignored ──
var Square = (n: number): number => n * n
assert_equal(25, Square(5))
var Concat = (a: string, b: string): string => a .. b
assert_equal('foobar', Concat('foo', 'bar'))

# ── an arrow lambda stored in a variable, called directly or via call() ──
var Add = (a, b) => a + b
assert_equal(7, Add(3, 4))
assert_equal(7, call(Add, [3, 4]))

# ── no-argument lambda, invoked immediately ──
assert_equal(42, ((): number => 42)())

# ── a lambda whose body is itself a method chain with a nested lambda ──
assert_equal([3, 6], [[1, 2], [3, 3]]->mapnew((_, pair) => pair->reduce((a, b) => a + b, 0)))

# ── demo ──
echo 'squares :' [1, 2, 3, 4, 5]->mapnew((_, v) => v * v)
echo 'sum     :' range(1, 100)->reduce((a, v) => a + v, 0)

# ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) .. ' assertion(s) failed'
endif
echo 'OK: vim9_lambda assertions passed'
