" Loop control: :for over the three iterable kinds, :while, :break, :continue,
" and the loop variable's lifetime after the loop ends.
let s = ''
for i in [1, 2, 3]
  let s .= i
endfor
echo s
" A string iterates by CHARACTER, and a dict by KEY in sorted order.
let s = ''
for c in split('abc', '\zs')
  let s .= c . '-'
endfor
echo s
let d = {'b': 2, 'a': 1, 'c': 3}
let ks = []
for k in sort(keys(d))
  call add(ks, k . '=' . d[k])
endfor
echo join(ks, ',')
" :for with a LIST pattern destructures each element.
let out = []
for [a, b] in [[1, 'x'], [2, 'y']]
  call add(out, a . b)
endfor
echo out
" break and continue
let n = 0
for i in range(10)
  if i == 2
    continue
  endif
  if i == 5
    break
  endif
  let n += i
endfor
echo n
" :while with the same two
let i = 0
let acc = []
while 1
  let i += 1
  if i % 2 == 0
    continue
  endif
  if i > 7
    break
  endif
  call add(acc, i)
endwhile
echo acc
" The loop variable survives the loop, holding its last value.
for last in [10, 20, 30]
endfor
echo last
" An empty iterable runs the body zero times and leaves the variable unset.
let ran = 0
for z in []
  let ran += 1
endfor
echo ran
echo exists('z')
" Nested loops with a labelled-ish break: :break leaves only the inner loop.
let pairs = []
for i in [1, 2]
  for j in ['a', 'b']
    if j ==# 'b'
      break
    endif
    call add(pairs, i . j)
  endfor
endfor
echo pairs
