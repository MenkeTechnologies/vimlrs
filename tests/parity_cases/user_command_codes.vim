" uc_check_code: <q-…> double-quotes with \ escapes, <f-…> splits (one
" argument under -nargs=1/?), <bang>, <line1>/<line2>/<range>/<count> and the
" uc_def default, <lt>, and an unknown <code> keeps its <.
command! -nargs=* -bang Hello echo 'hello' <q-args> <bang>0 <q-bang>
Hello a b
Hello! c
Hello
command! -nargs=1 Twice echo <args> * 2
Twice 21
command! -range=% Rng echo <line1> <line2> <range> <count>
Rng
command! -count=5 Cnt echo <count> <q-count> <line1>
Cnt
command! -complete=file -nargs=? F echo [<f-args>]
F x y
command! -nargs=* G echo [<f-args>]
G a "b" c\ d e\\f
G
command! -nargs=* Q echo <q-args>
Q it's "quoted" back\slash
command! -nargs=0 L echo '<lt>tag>' '<nope>' '<args'
L
command! -nargs=* M echo <q-mods> <q-reg> 'x'
M
command! -range=% Rng echo [<q-line1>, <q-line2>, <q-range>, <q-count>]
Rng
command! -range Rn2 echo [<q-line1>, <q-line2>, <q-range>, <q-count>]
Rn2
command! -range=3 Rn3 echo [<q-line1>, <q-line2>, <q-range>, <q-count>]
Rn3
command! -count Cn echo [<q-line1>, <q-line2>, <q-range>, <q-count>]
Cn
command! Pl echo [<q-line1>, <q-line2>, <q-range>, <q-count>]
Pl
