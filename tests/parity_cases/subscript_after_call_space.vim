" A call skips the white space after its `)` (get_func_tv), so a `[` there is
" still a subscript of what the call returned. Other bases keep the rule that a
" spaced `[` starts a new expression.
echo toupper('xy') [1]
echo 'ab'->toupper() [0]
echo range(5)->map({_, v -> v * 2}) [1:2]
echo function('toupper')('cd') [1]
echo {-> 'xyz'}() [2]
let d = {'f': {-> [4, 5]}}
echo d.f() [1]
echo [[7, 8]][0] [1]
echo (['a', 'b']) [1]
echo len([1, 2]) [9, 8][0]
echo 'after'
