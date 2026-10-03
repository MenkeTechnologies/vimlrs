" c: ex_let decides between ASSIGNING and LISTING by the text right after the
" target (skip_var_list + skipwhite): `=`, `op=` or `..=` assigns, anything else
" lists the variables, and a list argument that is not a name is E15 over the
" rest of the line (`get_name_len`), with nothing after it on the line run.
let y = 5
let z = 6
let y <<= 2
let y >>= 1
let y < 2
let y z ~ w
let y ~ z
let y <= 2
let y z<
let y ! = 3
let y 3
let y z | echo 'bar'
let y < 2 | echo 'not run'
let y nope z
let &ts
let $NOPE_X
let @a
let y &ts
let y 'str'
let y(
let y.
let y " comment
let y | echo 'ok'
let [y, z] < 2
echo y z
