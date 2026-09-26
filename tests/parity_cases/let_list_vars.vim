" :let {var-name} … lists each named variable (list_arg_vars/list_one_var_a):
" name, a space, padding to column 22, a type marker (# Number, * Funcref,
" [ List, { Dict, space otherwise), the value, and () after a Funcref.
let n = 42
let s = 'text'
let f = 1.5
let l = [1, 'two', [3]]
let d = {'k': 'v'}
let F = function('len')
let P = function('get', [{}])
let b = v:true
let e = []
let g:glob = -7
let n
let s f l d
let F P b e
let g:glob
let l[1] d.k d['k']
let averyveryverylongname_x = 'x'
let averyveryverylongname_x
let s " trailing comment
echo execute('let n s')
function! Show(a)
  let loc = a:a * 2
  let loc a:a
endfunction
call Show(5)
" An undefined name is E121, and the names after it are not listed.
let n nosuch s
echo 'after'
try
  let nosuch2
catch
  echo v:exception
endtry
