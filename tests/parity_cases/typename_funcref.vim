" typename() of a Funcref. A builtin's signature is recorded verbatim from vim
" (scripts/gen_builtin_signatures.sh); a legacy :function is always
" `func(...): any`; a lambda's shape is its declared parameter count.
function! UF(a, b)
  return 1
endfunction
let L0 = {-> 1}
let L1 = {x -> x}
let L2 = {x, y -> x}

" Builtins: argument count plus return type, every argument `[unknown]`.
echo typename(function('strlen'))
echo typename(function('add'))
echo typename(function('has'))
echo typename(function('argv'))
echo typename(function('tr'))
echo typename(function('sort'))
echo typename(function('empty'))
echo typename(function('function'))
echo typename(function('typename'))
" A partial over a builtin has no ufunc to read a type from.
echo typename(function('has', [1]))
echo typename(function('strlen', {}))

" A legacy :function is untyped whatever its arity, bound or not.
echo typename(function('UF'))
echo typename(funcref('UF'))
echo typename(function('UF', [1]))

" Lambdas: declared parameter count, minus what a partial has bound.
echo typename(L0)
echo typename(L1)
echo typename(L2)
echo typename({x, y, z -> x})
echo typename(function(L0, [1]))
echo typename(function(L1, [1]))
echo typename(function(L2, [1]))
echo typename(function(L2, [1, 2]))
echo typename(function(L2, [1, 2, 3]))

" A lambda that captures still reports its own declared parameters.
function! Cap()
  let a = 5
  echo typename({x -> x + a})
  echo typename({x, y -> x + a})
endfunction
call Cap()
