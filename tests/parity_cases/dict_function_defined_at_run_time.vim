" :function d.key() inside a function body or a block defines an anonymous
" function and stores it in the Dict when the line runs.
function! Counter()
  let c = {'v': 0}
  function! c.next() dict
    let self.v += 1
    return self.v
  endfunction
  return c
endfunction
let c1 = Counter()
let c2 = Counter()
call c1.next()
echo c1.next() c2.next()
if 1
  let g:o = {}
  function g:o.hi()
    return 'hi'
  endfunction
endif
echo g:o.hi()
let s:m = {}
for k in ['a', 'b']
  let s:m[k] = {'k': k}
  function! s:m[k].name() dict
    return 'n' . self.k
  endfunction
endfor
echo s:m.a.name() s:m.b.name()
