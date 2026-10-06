" A `:` at the start of an operand is a variable name: get_id_len() keeps a
" leading colon, so `:abc` and `:` are names, never E15.
echo :abc
echo 1 + :x
echo 0 ? :x : 3
let x = :
echo [:]
echo 0 ?: 'z'
echo 1 ?: 'z'
echo 0 ? : 'z'
echo 0 ? ) : 'z'
echo 'end'
