" map() checks each item's lock (lockvar locks the items too), and the
" container is locked while the expression runs.
let l = [1, 2]
lockvar l
call map(l, 'v:val + 1')
echo l
let l2 = [1, 2]
lockvar 1 l2
call map(l2, 'v:val + 1')
echo l2
let d = {'a': 1}
lockvar d
call map(d, 'v:val + 1')
echo d
let l3 = [1, 2]
call map(l3, 'add(l3, 3)')
echo l3
let d2 = {'a': 1}
call map(d2, 'extend(d2, {"b": 2})')
echo d2
let l4 = [1, 2]
lockvar l4
echo mapnew(l4, 'v:val * 10')
