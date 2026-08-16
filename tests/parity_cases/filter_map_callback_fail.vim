" map()/filter()/mapnew() when the per-item callback FAILS to evaluate.
"
" c: `filter_map_one()` — `if (eval_expr_typval(expr, false, argv, 2, newtv)
" == FAIL) { goto theend; }` (`Src/nvim/eval/list.c:58`), and every caller's loop
" `break`s on that FAIL (c:308 list, c:119 dict, c:188 blob, c:242 string). The
" item that failed and every later one are left UNTOUCHED; `mapnew()` simply
" stops appending, so it yields the partial new container.
"
" An error the callback merely *reports* is a different thing: the value it
" recovered is used (`strlen([1])` is 0), so `map([1,2], 'strlen([1])')` really
" is `[0, 0]`. Everything is `silent!` so the record is the container, and so
" `did_emsg` stays clear — the c:311 `|| did_emsg` half of the break is not what
" is being pinned here.

let g:r = 'UNSET'
silent! let g:r = map([1,2], 'nosuchfn()')
echo 'map-unknown   =' string(g:r)

let g:r = 'UNSET'
silent! let g:r = map([1,2], 'strlen([1])')
echo 'map-reported  =' string(g:r)

" The callback FAILS at the second item only: the first is replaced, the rest
" keep their original values.
let g:r = 'UNSET'
silent! let g:r = map([1,2,3], 'v:val == 2 ? [] . "" : v:val * 10')
echo 'map-mid       =' string(g:r)

let g:r = 'UNSET'
silent! let g:r = filter([1,2,3], 'nosuchfn()')
echo 'filter-unknown=' string(g:r)

let g:r = 'UNSET'
silent! let g:r = map({'a':1,'b':2}, 'nosuchfn()')
echo 'map-dict      =' string(g:r)

" mapnew() never touches the source, and stops appending to the new List.
let g:r = 'UNSET'
silent! let g:r = mapnew([1,2], 'nosuchfn()')
echo 'mapnew        =' string(g:r)

let g:r = 'UNSET'
silent! let g:r = map('ab', 'nosuchfn()')
echo 'map-string    =' string(g:r)

let g:r = 'UNSET'
silent! let g:r = map(0z0102, 'nosuchfn()')
echo 'map-blob      =' string(g:r)

" sort()'s comparator is called through the same `call_func` boundary: an error
" inside it is not the enclosing eval's failure, so sort() still returns a List.
let g:r = 'UNSET'
silent! let g:r = sort([2,1], 'nosuchcmp')
echo 'sort-badcmp   =' string(g:r)
