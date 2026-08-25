" The a: scope: varargs, the read-only rules, and a:firstline/a:lastline.
"
" c: call_user_func (vendor/eval/userfunc.c:1065-1099) builds fc_l_avars — a:0,
" a:000 (a FIXED list, c:1084-1088), a:firstline and a:lastline (c:1090-1098),
" then the named parameters and a:1..a:N. Every item is DI_FLAGS_RO, so writing
" one is var_check_ro's E46 (vars.c:2857 → c:2947) while a NEW a: name never
" reaches that and is e_illvar at c:2882.

function! V(a, ...)
  echo 'a=' a:a 'a:0=' a:0 'a:000=' string(a:000) 'type=' type(a:000)
  if a:0 >= 1 | echo '  a:1=' string(a:1) | endif
  if a:0 >= 2 | echo '  a:2=' string(a:2) | endif
endfunction
call V(1)
call V(1, 2)
call V(1, 2, [3, 4])
call V(1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
echo '--1'

" a:0 counts only the EXTRA arguments, and a:000 holds exactly those.
function! Fixed(a, b, ...)
  echo 'a:0=' a:0 'a:000=' string(a:000)
endfunction
call Fixed(1, 2)
call Fixed(1, 2, 3)
echo '--2'

" a:firstline / a:lastline exist in every non-lambda frame.
function! Range()
  echo 'firstline=' a:firstline 'lastline=' a:lastline
  echo 'sorted keys(a:)=' string(sort(keys(a:)))
  echo 'has_key firstline=' has_key(a:, 'firstline') 'lastline=' has_key(a:, 'lastline')
endfunction
call Range()
echo '--3'

" Reading a: argument that was not supplied.
function! W(...)
  silent! echo string(a:1)
  echo 'unsupplied a:1 errmsg=' v:errmsg
  let v:errmsg = ''
endfunction
call W()
echo '--4'

" Writing an a: name: E46 for one that exists, E461 for one that does not.
function! RO(a)
  silent! let a:a = 5
  echo 'let a:a  errmsg=' v:errmsg | let v:errmsg = ''
  silent! let a:0 = 5
  echo 'let a:0  errmsg=' v:errmsg | let v:errmsg = ''
  silent! let a:zz = 5
  echo 'let a:zz errmsg=' v:errmsg | let v:errmsg = ''
  echo 'a:a still' a:a
endfunction
call RO(1)
echo '--5'

" a:000 is a FIXED list, not merely a read-only name.
function! FixedList(...)
  silent! let a:000[0] = 99
  echo 'a:000[0]= errmsg=' v:errmsg | let v:errmsg = ''
  silent! call add(a:000, 5)
  echo 'add(a:000) errmsg=' v:errmsg | let v:errmsg = ''
  echo 'a:000 still' string(a:000)
endfunction
call FixedList(1, 2)
echo '--6'

" Wrong argument counts.
silent! call V()
echo 'too few  errmsg=' v:errmsg | let v:errmsg = ''
function! Two(a, b)
endfunction
silent! call Two(1, 2, 3)
echo 'too many errmsg=' v:errmsg | let v:errmsg = ''
echo '--7'

" Optional parameters with defaults, and a:0 with them.
function! Opt(a, b = 10, ...)
  echo 'a=' a:a 'b=' a:b 'a:0=' a:0 'a:000=' string(a:000)
endfunction
call Opt(1)
call Opt(1, 2)
call Opt(1, 2, 3)
echo '--8'
