" compare.vim — comparison operators and their case-sensitivity suffixes.
"
" `==#`/`=~#`/etc. force a case-sensitive match; `==?`/`=~?`/etc. force a
" case-insensitive one (bare `==` follows 'ignorecase'). This exercises the full
" set so the comparison-operator dispatch stays correct. Self-checks.
"
"   vimlrs examples/compare.vim

" ── string equality with case suffixes ──
call assert_equal(1, 'ABC' ==? 'abc')
call assert_equal(0, 'ABC' ==# 'abc')
call assert_equal(1, 'abc' ==# 'abc')
call assert_equal(1, 'ABC' !=# 'abc')
call assert_equal(0, 'ABC' !=? 'abc')

" ── regex match with case suffixes ──
call assert_equal(1, 'FooBar' =~? 'foobar')
call assert_equal(0, 'FooBar' =~# 'foobar')
call assert_equal(1, 'FooBar' !~# 'foobar')

" ── ordering comparisons ──
call assert_equal(1, 5 > 3)
call assert_equal(1, 3 <= 3)
call assert_equal(0, 2 >= 5)

" ── identity (is / isnot) on containers ──
let a = [1, 2]
let b = a
let c = [1, 2]
call assert_equal(1, a is b)
call assert_equal(1, a isnot c)
call assert_equal(1, a == c)

" ── demo ──
echo 'HELLO ==? hello :' ('HELLO' ==? 'hello')
echo 'sorted ci       :' sort(['banana', 'Apple', 'cherry'], {x, y -> x ==? y ? 0 : (tolower(x) < tolower(y) ? -1 : 1)})

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: compare assertions passed'
