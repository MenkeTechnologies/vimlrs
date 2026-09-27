" A :let lval quoted in an error message runs from the target to the end of
" the source line, past any | and the commands after it (value_check_lock with
" TV_CSTRING over a name that points into the command line).
let m = [1, 2]
lockvar m
try | let m[0] = 5 | catch | echo v:exception | endtry
let m[1] = 3 | echo 'not reached'
let d = {'k': 1}
lockvar d
if 1 | let d.k = 2 | endif
let m[0:1] = [7, 8] | echo 'not reached either'
let m[0] = 4
echo m d
