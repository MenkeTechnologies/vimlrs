" substitute() with a Funcref {sub}. c: f_substitute takes `expr = &argvars[2]`
" when tv_is_func() and vim_regsub_both() CALLS it per match. It used to be read
" as text, so a lambda substituted its own name (`he<lambda>1<lambda>1o`) and a
" Partial was E729.
"
" The argument is the List of ten submatches, supplied by fill_submatch_list()
" only to a user function that can take one more argument than the partial
" binds (a lambda always can: it is uf_varargs). A builtin has no ufunc_T, so it
" receives the List unfilled — empty. The result is read with
" tv_get_string_buf_chk: Number/Float/Bool render, List/Dict/Funcref are the
" per-type string error and contribute nothing.

function! Up(m)
  return toupper(a:m[0])
endfunction
echo substitute('hello', 'l', function('Up'), 'g')
echo substitute('hello', 'l', funcref('Up'), '')
let F = {m -> m[0] . '!'}
echo substitute('hello', 'l', F, 'g')
echo substitute('hello', 'l', {m -> m[0] . '!'}, 'g')
echo substitute('hello', '\(l\)\(l\)', {m -> m[2] . m[1] . len(m)}, '')
echo substitute('abc', '\(x\)\=b', {m -> string(m)}, '')
echo substitute('a1b2', '\d', {m -> m[0] * 2}, 'g')
echo substitute('abc', 'b', {m -> submatch(0) . 'S'}, '')
echo substitute('hello', 'l', {-> 'X'}, 'g')

" builtins get an empty List
echo substitute('abc', 'b', function('len'), '')
echo substitute('abc', 'b', function('string'), '')

" arity: varargs, partials, too many / too few
function! V(...)
  return a:0 . ':' . a:1[0]
endfunction
echo substitute('abc', 'b', function('V'), '')
echo substitute('abc', 'b', function('V', ['p']), '')
function! P(x, m)
  return a:x . a:m[0]
endfunction
echo substitute('abc', 'b', function('P', ['p']), '')
function! Z()
  return 'z'
endfunction
echo substitute('abc', 'b', function('Z'), '')
echo substitute('abc', 'b', function('Z', ['q']), '')
function! Two(a, b)
  return 'two'
endfunction
echo substitute('abc', 'b', function('Two'), '')
let d = {'v': 'D'}
function! d.f(m) dict
  return self.v
endfunction
echo substitute('abc', 'b', d.f, '')

" the result is rendered by tv_get_string_buf_chk
echo substitute('abc', 'b', {m -> 12}, '')
echo substitute('abc', 'b', {m -> 1.5}, '')
echo substitute('abc', 'b', {m -> v:true}, '')
echo substitute('abc', 'b', {m -> [1]}, '')
echo substitute('abc', 'b', {m -> {}}, '')
echo substitute('abc', 'b', {m -> function('len')}, '')
echo substitute('abc', 'b', {m -> "x\\y"}, '')
echo substitute('abc', 'b', {m -> "&"}, '')
echo 'done'
