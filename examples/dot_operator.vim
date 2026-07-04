" dot_operator.vim — the '.' operator's runtime subscript-vs-concat dispatch.
"
" A no-space `base.name` is syntactically identical whether it is a Dict
" subscript (`d.key`) or string concatenation (`a.b` with two string vars).
" Vim decides by the RUNTIME type of `base`: a Dict → member subscript, anything
" else → `.`-concatenation (the RHS `name` is a bare variable read). vimlrs lowers
" this to a bytecode type test that dispatches at execution time, single-evaluating
" `base` so side effects fire once and chains like `a.b.c` do not blow up.
"
"   vimlrs examples/dot_operator.vim

" --- concat: literals, numbers, and STRING VARIABLES (the runtime-dispatch case)
call assert_equal('ab', 'a'.'b')
call assert_equal('12', 1 . 2)
let a = 'p'
let b = 'q'
call assert_equal('pq', a.b)

" --- concat chains: every '.' concatenates when the bases are strings
let c = 'r'
call assert_equal('pqr', a.b.c)

" --- Dict subscript: the same syntax reads a member when base is a Dict
let d = {'x': 5, 'f': 1, 'g': 2}
call assert_equal(5, d.x)
call assert_equal(3, d.f + d.g)
let nested = {'a': {'b': 7}}
call assert_equal(7, nested.a.b)

" --- Dict subscript then a spaced concat, and a subscript on the member result
call assert_equal('5z', d.x . 'z')
let lists = {'x': [10, 20]}
call assert_equal(20, lists.x[1])

" --- a member value used inside a concatenation
let who = {'name': 'fox'}
call assert_equal('hi fox', 'hi ' . who.name)

" --- lambda-body concat: `a.b` over the lambda params, folded by reduce()
call assert_equal('abc', reduce(['a', 'b', 'c'], {x, y -> x.y}, ''))

" --- base is evaluated exactly once for both branches (no double-eval).
let g:calls = 0
function! Dictbase() abort
  let g:calls += 1
  return {'x': 42}
endfunction
call assert_equal(42, Dictbase().x)
call assert_equal(1, g:calls)

let g:calls = 0
function! Strbase() abort
  let g:calls += 1
  return 'p'
endfunction
let tail = 'q'
call assert_equal('pq', Strbase().tail)
call assert_equal(1, g:calls)

" --- self-test epilogue ---
if len(v:errors) > 0
  for err in v:errors
    echo 'FAIL:' err
  endfor
  throw 'dot_operator.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'dot_operator.vim: all assertions passed'
