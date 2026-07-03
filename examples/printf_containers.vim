" printf_containers.vim — printf('%s', …) / printf('%S', …) stringify containers.
" Vim's vsnprintf fetches a `%s`/`%S` argument through tv_str(), which returns
" encode_tv2echo() for a non-string typval — so a List/Dict/Funcref renders as its
" string() form ([1, 2, 3], {'a': 1}, type) instead of raising E730. `%S` differs
" only in that width/precision count screen cells; the value renders identically.
" Every expected value below was confirmed against vim 9.2 and nvim 0.12.3.

" --- %s: List, nested List, Dict, mixed, inner-string quoting
call assert_equal('[1, 2, 3]', printf('%s', [1, 2, 3]))
call assert_equal('[1, [2, 3], {''x'': ''y''}]', printf('%s', [1, [2, 3], {'x': 'y'}]))
call assert_equal('{''a'': 1}', printf('%s', {'a': 1}))
call assert_equal('[''a'', ''b'']', printf('%s', ['a', 'b']))

" --- %s: scalars unchanged (plain string stays unquoted, no wrapper on funcref)
call assert_equal('foo', printf('%s', 'foo'))
call assert_equal('42', printf('%s', 42))
call assert_equal('3.14', printf('%s', 3.14))
call assert_equal('type', printf('%s', function('type')))
call assert_equal('[bar]', printf('[%s]', 'bar'))

" --- %S: identical value rendering to %s
call assert_equal('[1, 2, 3]', printf('%S', [1, 2, 3]))
call assert_equal('{''a'': 1}', printf('%S', {'a': 1}))
call assert_equal('[''a'', ''b'']', printf('%S', ['a', 'b']))
call assert_equal('foo', printf('%S', 'foo'))
call assert_equal('42', printf('%S', 42))
call assert_equal('3.14', printf('%S', 3.14))
call assert_equal('type', printf('%S', function('type')))

" --- width / precision apply to the stringified container
call assert_equal('    [1, 2]', printf('%10s', [1, 2]))
call assert_equal('[1, 2]    |', printf('%-10s|', [1, 2]))
call assert_equal('[1, 2', printf('%.5s', [1, 2, 3]))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'printf_containers.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'printf_containers.vim: all assertions passed'
