" list_unpack.vim — :let list destructuring, incl. the [head; rest] form.
"
" :let [a, b, c] = list binds several names at once; a trailing ;name in the
" target list binds the remainder as a List. This is the lvalue grammar that
" skip_var_list() (vars.c) parses. Self-tests with assert_*; exits non-zero on
" any failure.
"
"   vimlrs examples/list_unpack.vim

" ── exact-length unpack ──
let [a, b, c] = [1, 2, 3]
call assert_equal(1, a)
call assert_equal(2, b)
call assert_equal(3, c)

" ── swap via a list target (rhs is built before any binding) ──
let [a, b] = [b, a]
call assert_equal([2, 1], [a, b])

" ── [head; rest]: the rest name soaks up the remaining elements as a List ──
let [head; rest] = [10, 20, 30, 40]
call assert_equal(10, head)
call assert_equal([20, 30, 40], rest)

" ── leading names plus a rest binding ──
let [first, second; tail] = ['p', 'q', 'r', 's']
call assert_equal('p', first)
call assert_equal('q', second)
call assert_equal(['r', 's'], tail)

" ── a rest binding can match exactly zero remaining elements ──
let [only; nothing] = [99]
call assert_equal(99, only)
call assert_equal([], nothing)

" ── unpacking works element-wise over heterogeneous values ──
let [name, attrs] = ['root', #{uid: 0}]
call assert_equal('root', name)
call assert_equal(0, attrs.uid)

" ── a common idiom: destructure inside a :for over pairs ──
let total = 0
for [k, v] in items(#{a: 1, b: 2, c: 3})
  let total += v
endfor
call assert_equal(6, total)

" ── demo ──
let [d, e; f] = range(1, 5)
echo '[d, e; f] = range(1,5) ->' d e f

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'list_unpack.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
