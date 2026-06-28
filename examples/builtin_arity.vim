" builtin_arity.vim — builtin functions reject wrong argument counts.
"
" Vim validates a builtin call's argument count against its funcs[] table
" (generated from eval.lua) before the function runs: too few arguments is
" E119, too many is E118. vimlrs mirrors that table so a mis-arity call is a
" clean error on both the direct-call and call()/Funcref dispatch paths,
" instead of crashing in the function body. Self-tests into v:errors.
"
"   vimlrs examples/builtin_arity.vim

" ── correct arity: these all succeed ──
call assert_equal(2, get([1, 2], 1))
call assert_equal(99, get([1, 2], 5, 99))
call assert_equal([1, 2, 3], add([1, 2], 3))
call assert_equal(3, abs(-3))
call assert_equal('7', printf('%d', 7))
call assert_equal('1-2-3', printf('%d-%d-%d', 1, 2, 3))

" ── too FEW arguments → E119 (get needs 2, abs needs 1) ──
call assert_fails('call get([])', 'E119')
call assert_fails('call abs()', 'E119')
call assert_fails('call add([1])', 'E119')

" ── too MANY arguments → E118 (abs/len take exactly 1) ──
call assert_fails('call abs(1, 2)', 'E118')
call assert_fails('call len([1], 2)', 'E118')

" ── the same guard applies to the dynamic call()/Funcref path ──
call assert_equal('5', call('printf', ['%d', 5]))
call assert_fails("call call('get', [[]])", 'E119')
let F = function('add')
call assert_equal([9, 8], F([9], 8))

" ── variadic functions accept any count past their minimum (printf: >= 1) ──
call assert_equal('a', printf('a'))
call assert_equal('a b c', printf('%s %s %s', 'a', 'b', 'c'))

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: builtin_arity assertions passed'
