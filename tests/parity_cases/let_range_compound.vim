" A compound operator on l[i:j] is applied item by item (c: list_assign_range
" with op): items before an error keep their new value.
let l = [1, 2]
let l[0:1] += [1]
echo l
let l = [1, 2, 3]
let l[0:1] = [7, 8, 9]
echo l
let l = [1, 2, 3]
let l[0:1] = [7]
echo l
let l = [1, 2, 3]
let l[1:] -= [1, 1]
echo l
let l = [1, 2]
let l[1:] += [5, 6, 7]
echo l
let l = ['a', 'b']
let l[0:1] .= ['x', 'y']
echo l
let l = [1, 2]
let l[0:0] *= [3]
echo l
let l = [1, [2]]
let l[0:1] += [1, 1]
echo l
