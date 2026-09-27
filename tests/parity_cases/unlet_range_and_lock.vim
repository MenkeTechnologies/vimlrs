" :unlet of a List range, and the container lock checked before an element goes.
" Both engines agree on every line; vim 9.2 and Neovim word E689 differently,
" so no E689 case is recorded here.
let l = [0, 1, 2, 3, 4, 5]
unlet l[1:2]
echo l
unlet l[-2:]
echo l
let l = [0, 1, 2, 3]
unlet l[:1]
echo l
let l = [0, 1, 2, 3]
unlet l[1:10]
echo l
let l = [0, 1, 2, 3]
unlet l[3:1]
echo l
let l = [0, 1, 2, 3]
unlet l[5:]
echo l
let l = [0, 1, 2, 3]
unlet l[-9:1]
echo l
let l = [0, 1, 2, 3]
unlet l[1:-9]
echo l
let l = [0, 1, 2, 3]
unlet l[:]
echo l
let d = {'a': 1}
unlet d['a':'b']
echo d
let b = 0z01020304
unlet b[1:2]
echo b
unlet b[1]
echo b
let nest = {'x': [1, 2, 3]}
unlet nest.x[1:]
unlet nest['x'][0:0]
echo nest
let g:gl = [1, 2, 3]
unlet g:gl[0:1]
echo g:gl
let m = [1, 2, 3]
lockvar m
unlet m[0]
unlet m[0:1]
echo m
let md = {'a': 1}
lockvar md
unlet md.a
echo md
let t = [1, 2, 3]
lockvar t
" A lval in an error message runs to the end of the source line.
try | let t[0] = 9 | catch | echo 'caught' v:exception | endtry
let t[1] = 8 | echo 'not reached'
" Once an :unlet argument fails, the rest are not unlet.
unlet nosuch t[0]
let u = [1, 2]
unlet u[5] u[0]
echo u
unlet! nosuch2 u[0]
echo u
echo 'end'
