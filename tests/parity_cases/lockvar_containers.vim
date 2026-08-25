" :lockvar depth, and what each depth refuses.
"
" c: set_var_lval checks value_check_lock BEFORE applying the operator
" (vendor/eval/typval.c:1921 for the message, `E741: Value is locked: %s` /
" `E742: Cannot change value of %s`), and the name it prints is lp->ll_name —
" which get_lval leaves pointing INTO the source when the lval is subscripted.
" f_add carries the same guard (vendor/eval/list.c:434, "add() argument").

function! Show(what, val)
  echo a:what . ' = ' . string(a:val) . '  errmsg=[' . v:errmsg . ']'
  let v:errmsg = ''
endfunction
let v:errmsg = ''

" ---- default depth (2): the variable AND its contents ----
let g:l = [1, 2, 3]
lockvar g:l
call Show('islocked g:l', islocked('g:l'))
call Show('islocked g:l[0]', islocked('g:l[0]'))
silent! let g:l[0] = 9
call Show('l[0] =', g:l)
silent! let g:l += [4]
call Show('l +=', g:l)
silent! let g:l = [7]
call Show('l =', g:l)
silent! call add(g:l, 4)
call Show('add(l)', g:l)
silent! call remove(g:l, 0)
call Show('remove(l)', g:l)
silent! let g:l[0:1] = [8,8]
call Show('l[0:1] =', g:l)
unlockvar g:l
silent! let g:l[0] = 9
call Show('after unlock', g:l)

let g:d = {'a': 1}
lockvar g:d
silent! let g:d.a = 9
call Show('d.a =', g:d)
silent! let g:d.b = 9
call Show('d.b = (new key)', g:d)
silent! let g:d['a'] = 9
call Show("d['a'] =", g:d)
silent! call extend(g:d, {'c': 3})
call Show('extend(d)', g:d)
unlockvar g:d

let g:bl = 0z0102
lockvar g:bl
silent! call add(g:bl, 3)
call Show('add(blob)', g:bl)
unlockvar g:bl

" ---- depth 1: the variable only, contents stay writable ----
let g:l2 = [1, 2, 3]
lockvar 1 g:l2
call Show('depth1 islocked g:l2', islocked('g:l2'))
call Show('depth1 islocked g:l2[0]', islocked('g:l2[0]'))
silent! let g:l2[0] = 7
call Show('depth1 l2[0] =', g:l2)
silent! call add(g:l2, 4)
call Show('depth1 add(l2)', g:l2)
silent! let g:l2 = [0]
call Show('depth1 l2 =', g:l2)
unlockvar 1 g:l2

" ---- depth 2 on a nested container ----
let g:n = [[1], [2]]
lockvar 2 g:n
silent! let g:n[0] = [9]
call Show('depth2 n[0] =', g:n)
silent! let g:n[0][0] = 9
call Show('depth2 n[0][0] =', g:n)
unlockvar g:n
lockvar 3 g:n
silent! let g:n[0][0] = 9
call Show('depth3 n[0][0] =', g:n)
unlockvar g:n

" ---- :const is a lock too ----
const g:c = [1]
silent! let g:c = [2]
call Show('const reassign', g:c)
silent! let g:c[0] = 2
call Show('const item', g:c)

" A `:let` whose lval is SUBSCRIPTED is named in the message by lp->ll_name,
" which get_lval leaves pointing into the source — so the message quotes the
" whole remaining command, not just the variable.
let g:q = [1]
lockvar g:q
silent! let g:q[0] = 'quoted tail'
call Show('subscripted lval name', v:errmsg)
silent! let g:q = 'plain'
call Show('plain lval name', v:errmsg)
unlockvar g:q
