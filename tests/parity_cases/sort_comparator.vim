" sort()/uniq() with a comparator, and what makes one FAIL.
"
" c: item_compare2 (vendor/eval/typval.c:1279) calls the comparator and sets
" item_compare_func_err when call_func returned FAIL (c:1314-1317) or when the
" result is not a Number (c:1319); do_sort_uniq then reports `E702: Sort compare
" function failed` (c:1382-1383) and leaves the List alone.
"
" call_func FAILS for a LAMBDA whose body failed to evaluate — a lambda body is
" an expression, not a do_cmdline, so nothing swallows the failure the way a
" :function body does. That is the difference between the two error rows below:
" an unknown comparator NAME and a comparator whose BODY errors both end at
" E702, by two different routes.

echo 'default    ' string(sort([3,1,2]))
echo 'numeric    ' string(sort([10,9,2], 'n'))
echo 'string     ' string(sort(['b','A','a'], 1))
echo 'icase      ' string(sort(['b','A','a'], 'i'))
echo 'lambda     ' string(sort([3,1,2], {a,b -> a - b}))
echo 'lambda rev ' string(sort([3,1,2], {a,b -> b - a}))
function! Cmp(a, b)
  return a:a - a:b
endfunction
echo 'funcname   ' string(sort([3,1,2], 'Cmp'))
echo 'funcref    ' string(sort([3,1,2], function('Cmp')))
echo 'stable     ' string(sort([[1,'a'],[1,'b'],[0,'c']], {a,b -> a[0] - b[0]}))
echo 'dict arg   ' string(sort([3,1,2], {a,b -> a - b}, {}))
let v:errmsg = ''
silent! echo string(sort([3,1,2], 'nosuchcmp'))
echo 'badcmp err ' v:errmsg
let v:errmsg = ''
silent! echo string(sort([3,1,2], {a,b -> [] . ''}))
echo 'errcmp     ' v:errmsg
let v:errmsg = ''
echo 'uniq       ' string(uniq([1,1,2,2,3]))
echo 'uniq cmp   ' string(uniq([1,2,3,4], {a,b -> (a % 2) - (b % 2)}))
echo 'sort f     ' string(sort([1.5, 0.5, 1.0], 'f'))
echo 'sort l     ' string(sort(['b','A','a'], 'l'))
echo 'sort N     ' string(sort(['x10','x9'], 'N'))
echo '--1'

" A comparator whose body merely REPORTS an error but still yields a value does
" not fail the call — call_func returns OK with the recovered rettv.
echo 'reported   ' string(sort([3,1,2], {a,b -> strlen([1])}))
echo 'reported v ' v:errmsg
let v:errmsg = ''

" A comparator that returns a non-Number is the other half of c:1319.
silent! echo string(sort([3,1,2], {a,b -> [1,2]}))
echo 'nonnumber  ' v:errmsg
let v:errmsg = ''
silent! echo string(sort([3,1,2], {a,b -> 'x'}))
echo 'string ret ' v:errmsg
let v:errmsg = ''

" A :function comparator that errors internally does NOT fail the call.
function! Noisy(a, b)
  echo strlen([1])
  return a:a - a:b
endfunction
silent! echo 'fn noisy   ' string(sort([3,1,2], 'Noisy'))
echo 'fn noisy v ' v:errmsg
let v:errmsg = ''

" uniq() shares the machinery.
silent! echo string(uniq([1,1,2], {a,b -> [] . ''}))
echo 'uniq err   ' v:errmsg
echo '--2'

" The same matrix under :silent!, where did_emsg stays clear — which is what
" separates "the body reported an error" from "the body failed to evaluate".
function! Show2(t)
  echo a:t . ' [' . v:errmsg . ']'
  let v:errmsg = ''
endfunction
let v:errmsg = ''
let g:r = 'X' | silent! let g:r = sort([3,1,2], {a,b -> strlen([1])})
call Show2('silent reports ' . string(g:r))
let g:r = 'X' | silent! let g:r = sort([3,1,2], {a,b -> [] . ''})
call Show2('silent fails   ' . string(g:r))
let g:r = 'X' | silent! let g:r = sort([3,1,2], {a,b -> [1]})
call Show2('silent nonnum  ' . string(g:r))
let g:r = 'X' | silent! let g:r = sort([3,1,2], {a,b -> a - b})
call Show2('silent ok      ' . string(g:r))
let g:r = 'X' | silent! let g:r = sort([3,1,2], 'Noisy')
call Show2('silent fn noisy' . string(g:r))
echo '--3'
