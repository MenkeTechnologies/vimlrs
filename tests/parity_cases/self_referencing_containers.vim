" A List or Dict that holds itself: copying and encoding it must terminate.
" (How :echo/string() RENDER the back-reference is an engine split — vim
" `[...]`, Neovim and this port `[...@0]` — see examples/dict_deepcopy.vim.)
let l = [1]
call add(l, l)
let d = {'a': 1}
let d.self = d
echo len(l) l[1] is l d.self is d
" deepcopy() copies each container once and points the copy at the copy.
let c = deepcopy(l)
echo c[1] is c c is l c[1] is l
let e = deepcopy(d)
echo e.self is e e is d
let x = [1, 2]
let y = deepcopy([x, x])
echo y[0] is y[1]
let y2 = deepcopy([x, x], 1)
echo y2[0] is y2[1]
" With {noref} nothing is shared, and the recursion stops at the nesting cap.
let n = deepcopy(l, 1)
echo 'after noref'
" json_encode() writes the back-reference as an empty container.
echo json_encode(l) json_encode({'k': d})
echo l == l d == d
echo 'end'
