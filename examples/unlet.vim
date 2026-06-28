" unlet.vim — :unlet of variables, List items, and Dict entries.
"
" Demonstrates that :unlet removes not just bare variables but also a single
" List item (`unlet l[i]`) or Dict entry (`unlet d.key` / `unlet d['key']`),
" mirroring do_unlet_var() in Vim. Self-tests into v:errors; exits non-zero on
" failure.
"
"   vimlrs examples/unlet.vim

" ── bare variable ──
let x = 42
call assert_true(exists('x'))
unlet x
call assert_false(exists('x'))

" ── Dict entry: d.key and d['key'] ──
let d = {'a': 1, 'b': 2, 'c': 3}
unlet d.a
call assert_equal({'b': 2, 'c': 3}, d)
unlet d['c']
call assert_equal({'b': 2}, d)

" ── List item: positive and negative index ──
let l = [10, 20, 30, 40]
unlet l[1]
call assert_equal([10, 30, 40], l)
unlet l[-1]
call assert_equal([10, 30], l)

" ── nested container — the inner Dict is a shared reference ──
let tree = {'left': {'v': 1, 'tmp': 9}, 'right': [7, 8, 9]}
unlet tree.left.tmp
call assert_equal({'v': 1}, tree.left)
unlet tree.right[0]
call assert_equal([8, 9], tree.right)

" ── several targets in one :unlet (names and elements mixed) ──
let keep = 'me'
let cfg = {'on': 1, 'off': 0}
unlet keep cfg.off
call assert_false(exists('keep'))
call assert_equal({'on': 1}, cfg)

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: unlet assertions passed'
