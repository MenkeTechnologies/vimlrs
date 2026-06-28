" oneline.vim — one-line block bars: if/while/for on a single line with `|`.
"
" A block opener and its body/terminator can share a line, separated by `|`
" (Vim's one-line block syntax), including after a leaf command. Self-checks.
"
"   vimlrs examples/oneline.vim

" ── one-line if / else, used as an expression-ish guard ──
let g:msg = ''
if 1 | let g:msg = 'yes' | endif
call assert_equal('yes', g:msg)

let x = 5
if x > 3 | let g:size = 'big' | else | let g:size = 'small' | endif
call assert_equal('big', g:size)

" ── a leaf command, then a one-line block, on the same line ──
let total = 0 | for n in [1, 2, 3, 4] | let total += n | endfor
call assert_equal(10, total)

" ── one-line while ──
let i = 0 | let acc = []
while i < 3 | call add(acc, i * i) | let i += 1 | endwhile
call assert_equal([0, 1, 4], acc)

" ── nested one-line blocks ──
let evens = []
for n in range(1, 6) | if n % 2 == 0 | call add(evens, n) | endif | endfor
call assert_equal([2, 4, 6], evens)

" ── demo ──
echo 'evens:' evens 'total:' total

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: oneline assertions passed'
