" A lambda's named arguments go straight into its l: dict (call_user_func),
" not through set_var_const, so a Funcref argument under a lowercase name is
" callable and is not E704.
echo map([function('len')], {i, v -> v([1, 2])})
echo map([function('len')], {_, f -> f('abc')})
echo filter([{-> 0}, {-> 1}], {_, g -> g()})
echo call({f, x -> f(x)}, [function('toupper'), 'ab'])
let F = {cb -> cb()}
echo F({-> 'ok'})
try | let s = function('len') | catch | echo v:exception | endtry
