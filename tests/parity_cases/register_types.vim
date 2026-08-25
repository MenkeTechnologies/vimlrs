" Register contents and their MOTION TYPE, through setreg()/getreg()/
" getregtype() and `let @x =`.
"
" c: get_reg_type (vendor/ops.c:235) answers kMTUnknown for a register that was
" never set (`y_array == NULL`, c:264) and for an invalid name (c:252), and
" format_reg_type renders that as the EMPTY string (c:228-230) — not as "v".
" setreg()'s {options} carry the type: "l"/"V" linewise, "b"/CTRL-V blockwise
" with an optional width, "a" appends, and a List argument is linewise by
" default. getreg(..., 1, 1) returns the lines as a List.

call setreg('a', 'plain')
echo 'char      ' string(getreg('a')) getregtype('a') string(getreg('a', 1, 1))
call setreg('b', "l1\nl2", 'l')
echo 'line      ' string(getreg('b')) getregtype('b') string(getreg('b', 1, 1))
call setreg('c', ['x','y'])
echo 'list      ' string(getreg('c')) getregtype('c') string(getreg('c', 1, 1))
call setreg('d', ['x','y'], 'b')
echo 'block     ' string(getreg('d')) getregtype('d') string(getreg('d', 1, 1))
call setreg('e', 'z', 'b5')
echo 'block5    ' string(getreg('e')) getregtype('e')
call setreg('f', 'a')
call setreg('f', 'b', 'a')
echo 'append    ' string(getreg('f')) getregtype('f')
call setreg('g', ['p'], 'al')
echo 'append l  ' string(getreg('g')) getregtype('g')
echo 'empty reg ' string(getreg('z')) '[' . getregtype('z') . ']'
call setreg('h', '')
echo 'set empty ' string(getreg('h')) '[' . getregtype('h') . ']'
echo '--1'
let @i = 'via let'
echo 'let @i    ' string(@i) getregtype('i')
let @j = "two\nlines"
echo 'let nl    ' string(@j) getregtype('j')
echo '--2'
echo 'reg_recording=' string(reg_recording()) ' reg_executing=' string(reg_executing())
echo '--3'

" An invalid register name has no type either.
echo 'invalid   ' "[" . getregtype('!') . "]"
" The read-only registers are always charwise.
echo 'readonly  ' getregtype('%') getregtype(':') getregtype('/') getregtype('.')
echo 'blackhole ' getregtype('_') string(getreg('_'))
" Setting a List with an explicit blockwise type keeps the width.
call setreg('k', ['ab','cd'], 'b2')
echo 'block w2  ' string(getreg('k')) getregtype('k') string(getreg('k', 1, 1))
" Appending to a linewise register keeps it linewise.
call setreg('l', ['a'], 'l')
call setreg('l', ['b'], 'a')
echo 'append lw ' string(getreg('l')) getregtype('l')
" setreg() with an empty List empties the register.
call setreg('m', ['x'])
call setreg('m', [])
echo 'empty list' string(getreg('m')) "[" . getregtype('m') . "]"
echo '--4'
