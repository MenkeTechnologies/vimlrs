" The `=` that ends a :let target is the first one outside every bracket pair and
" string — a comparison inside a subscript, a `=` in a quoted key, or a Dict
" literal on the right must not be taken for it. Likewise the `:` that makes a
" subscript a range is the first one not answering a `?`.
let l = [1, 2, 3, 4]
let l[(1 == 1)] = 'a'
let l[(2 != 3):(3 >= 2)] = ['b']
echo l
let d = {}
let d['a=b'] = 1
let d["c=d"] = 2
let d[(1 ==# 1) ? 'x' : 'y'] = 3
let d[0 ? 'p' : 'q'] = 4
echo d
let l[(0 =~ '0'):(1 ==? 1)] = ['z']
echo l
let [p, q] = [(1 == 1), {'k': (2 == 2)}]
echo p q
let e = {'k': [1, 2]}
let e.k[(1 == 1)] = 9
echo e
let l2 = [1, 2, 3, 4]
let l2[(1 is 1):] = [7, 8, 9]
echo l2
let l2[1 ? 0 : 2 :1] = [5, 6]
echo l2
unlet l2[1 ? 0 : 2]
echo l2
unlet l2[0 : 1 ? 1 : 2]
echo l2
echo l[1 ? 0 : 1 : 2]
