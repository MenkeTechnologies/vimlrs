" :unlet! resolves its target with GLV_QUIET: a missing Dict key, an index out
" of range, a base that is undefined or cannot be indexed — none is reported.
let d = {'a': 1}
unlet! d['b']
unlet! d.b
unlet! d['a'] d['b']
echo d
let l = [1, 2, 3]
unlet! l[5]
unlet! l[1:5]
unlet! l[7:8]
echo l
unlet! nosuch['a']
unlet! nosuch.a
let n = 1
unlet! n[0]
let e = {}
unlet! e.a.b
echo n e
" Without the bang every one of them reports.
unlet d['b']
unlet l[5]
unlet nosuch['a']
function! F()
  let x = {}
  unlet! x['k']
  unlet! y['k']
  echo 'in F'
endfunction
call F()
