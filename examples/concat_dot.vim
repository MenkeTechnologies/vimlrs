" concat_dot.vim — the '.' operator: string concatenation vs dict member access.
" Legacy Vimscript overloads '.': `a . b` (and `a.b`) concatenates, while `d.key`
" reads a Dict member. The disambiguation: `.name(` is always concatenation with
" a function call (legacy has no direct `dict.key(args)` call — that is vim9), so
" `f(x).g(y)` concatenates the two results. Self-test into v:errors.

" --- '.' concatenation, including with no surrounding spaces after a call
call assert_equal('ab', 'a' . 'b')
call assert_equal('ab', 'a'.'b')
call assert_equal('AB', toupper('a').toupper('b'))
call assert_equal('aabbcc', substitute('abc', '.', '\=submatch(0).submatch(0)', 'g'))
call assert_equal('aabc', substitute('abc', '.', '\=submatch(0).submatch(0)', ''))

" --- dict member access still works (no-space '.key' on a Dict variable)
let d = {'k': 5, 'f': 1, 'g': 2}
call assert_equal(5, d.k)
call assert_equal(3, d.f + d.g)
let nested = {'a': {'b': 7}}
call assert_equal(7, nested.a.b)

" --- a member value used in a concatenation
let who = {'name': 'fox'}
call assert_equal('hi fox', 'hi ' . who.name)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'concat_dot.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'concat_dot.vim: all assertions passed'
