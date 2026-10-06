" Each item of a :let/:for unpack is a full :let target (ex_let_vars ->
" ex_let_one), and the first item that fails ends the unpack.
let l = [1,2,3]
let [l[0], l[1]] = [9, 8] | echo l
let [l[0], l[1]] = [7, 6]
echo l
let d = {}
let [d.a, d['b']] = [1, 2]
echo d
let [x; l[2:]] = [0, 5]
echo l
let [g:a, $FOO, @r, &ts] = [1, 'e', 'r', 4]
echo g:a $FOO @r &ts
let d = {}
for [d.k, d.v] in [[1, 2], [3, 4]] | echo d | endfor
let l = [0, 0]
for [l[0], l[1]] in [[5, 6]] | endfor
echo l
let [a, b] = [1]
let [a, b] = [1, 2, 3]
let [s:x, g:y; g:z] = [1, 2, 3, 4] | echo s:x g:y g:z
const [c1, c2] = [1, 2] | echo c1 c2
let c1 = 5
let [d.a, d.a] = [1, 2] | echo d
let [l[5], b] = [1, 2]
echo b
let [v:count, b] = [1, 2]
let ll = [1, 2]
lockvar ll
echo ll
