" map()/filter()/mapnew()/foreach() when the callback REPORTS an error without
" failing the evaluator — the other half of filter_map_callback_fail.vim.
"
" c: after a successful filter_map_one() every loop tests `did_emsg` and breaks,
" discarding the value it just computed (vendor/eval/list.c:311 for the List,
" c:119 Dict, c:189 Blob, c:243 String). filter_map() clears did_emsg before the
" walk so the flag means "this callback errored" (c:380-383) and ORs the
" caller's back afterwards (c:401).
"
" filter_map_one() itself returns FAIL only when eval_expr_typval() did, which a
" LAMBDA callback never does — call_func() returns OK whatever the body
" reported. So this is the test that stops a lambda callback, and nothing else
" does. The sibling case pins the eval-FAIL half instead, under :silent!, where
" did_emsg deliberately stays clear.

echo 'list map    ' string(map([1,2], {i,v -> v . [1]}))
echo '--1'
echo 'list map mid' string(map([1,2,3], {i,v -> v == 2 ? [] . '' : v * 10}))
echo '--2'
echo 'list filter ' string(filter([1,2,3], {i,v -> v == 2 ? [] . '' : 1}))
echo '--3'
echo 'mapnew      ' string(mapnew([1,2], {i,v -> v . [1]}))
echo '--4'
echo 'dict map    ' string(map({'a':1}, {k,v -> v . [1]}))
echo '--5'
echo 'string map  ' string(map('ab', {i,v -> v . [1]}))
echo '--6'
echo 'blob map    ' string(map(0z0102, {i,v -> v . [1]}))
echo '--7'
echo 'unknown fn  ' string(map([1,2], {i,v -> nosuchfunction_xyz(v)}))
echo '--8'

" The same callbacks written as STRING expressions already stopped, because the
" expression itself fails to evaluate.
echo 'string cb   ' string(map([1,2], 'v:val . [1]'))
echo '--9'
echo 'string cb mid' string(map([1,2,3], 'v:val == 2 ? [] . "" : v:val * 10'))
echo '--10'

" A callback that only errors on a LATER item leaves the earlier results.
echo 'late fail   ' string(map([1,2,3,4], {i,v -> v == 3 ? [] . '' : v + 100}))
echo '--11'

" A callback that never errors is unaffected.
echo 'clean       ' string(map([1,2,3], {i,v -> v * 2}))
echo 'clean filter' string(filter([1,2,3,4], {i,v -> v % 2 == 0}))
echo '--12'

" foreach() runs for its side effects; the error stops it too. The failing
" operand is deliberately NOT a call argument here: an E116 raised inside a
" lambda body quotes the body text vim stored for the lambda, and this port
" quotes the enclosing parse buffer, so it would carry the trailing `})` too.
let g:seen = []
call foreach([1,2,3], {i,v -> v == 2 ? [] . '' : add(g:seen, v)})
echo 'foreach seen' string(g:seen)
echo '--13'
