" typename() of a List or Dict that contains itself: the walk meets the
" container again and answers list<any> / dict<any> for it (vim9type.c "avoid
" recursion"), so the outer member type is the common type of that and the rest.
let l = [1]
call add(l, l)
echo typename(l)
let d = {'a': 1}
let d.self = d
echo typename(d)
let n = [[2]]
call add(n[0], n)
echo typename(n)
let shared = [3]
echo typename([shared, shared])
