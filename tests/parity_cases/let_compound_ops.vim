" `:let x op= y` is NOT the expression operator. c: ex_let_one applies it with
" eexe_mod_op (vendor/eval/executor.c:201), a TYPE TABLE whose every miss is
" `E734: Wrong variable type for op=` and leaves the variable untouched — while
" `x op y` in an expression coerces. The env/register/option targets have their
" own guards in ex_let_env (vendor/eval/vars.c:1316), ex_let_register (c:1457)
" and ex_let_option (c:1379-1384).
"
" NOT probed here: `.=` with v:true / v:null on the right. Neovim rejects those
" (executor.c:206-210, `(VAR_BOOL || VAR_SPECIAL) && *op == '.'`); vim 9.2 has
" no such clause and concatenates. This port follows the vendored C.

function! Show(what, val)
  echo a:what . ' = ' . string(a:val) . '  errmsg=[' . v:errmsg . ']'
  let v:errmsg = ''
endfunction
let v:errmsg = ''

" ---- the combinations that WORK ----
let g:n = 10
let g:n += 5  | call Show('nr +=', g:n)
let g:n -= 3  | call Show('nr -=', g:n)
let g:n *= 2  | call Show('nr *=', g:n)
let g:n /= 4  | call Show('nr /=', g:n)
let g:n %= 4  | call Show('nr %=', g:n)
let g:n .= 'z'| call Show('nr .=', g:n)
let g:s = 'ab'
let g:s .= 'cd'  | call Show('str .=', g:s)
let g:s ..= 'ef' | call Show('str ..=', g:s)
let g:s2 = '7'
let g:s2 += 1 | call Show('str +=', g:s2)
let g:f = 1.5
let g:f += 0.25 | call Show('float +=', g:f)
let g:f -= 1    | call Show('float -= nr', g:f)
let g:f *= 2    | call Show('float *= nr', g:f)
let g:f /= 4.0  | call Show('float /=', g:f)
let g:n3 = 2
let g:n3 += 0.5 | call Show('nr += float', g:n3)
let g:l = [1,2]
let g:l += [3]  | call Show('list +=', g:l)
let g:l += []   | call Show('list += []', g:l)
let g:bl = 0z0102
let g:bl += 0z03 | call Show('blob +=', g:bl)
let g:d = {'a':1}
let g:d.a += 2  | call Show('dict member +=', g:d)
let g:li = [1,2]
let g:li[0] += 10 | call Show('list index +=', g:li)

" ---- division and modulo by zero (num_divide / num_modulus) ----
let g:z1 = 7  | let g:z1 /= 0 | call Show('nr /= 0', g:z1)
let g:z2 = -7 | let g:z2 /= 0 | call Show('-nr /= 0', g:z2)
let g:z3 = 0  | let g:z3 /= 0 | call Show('0 /= 0', g:z3)
let g:z4 = 7  | let g:z4 %= 0 | call Show('nr %= 0', g:z4)

" ---- the combinations that are E734 ----
silent! let g:e1 = [1]     | silent! let g:e1 *= [2]      | call Show('list *= list', g:e1)
silent! let g:e2 = [1]     | silent! let g:e2 += 1        | call Show('list += nr', g:e2)
silent! let g:e3 = 0z01    | silent! let g:e3 -= 0z02     | call Show('blob -= blob', g:e3)
silent! let g:e4 = 0z01    | silent! let g:e4 += 1        | call Show('blob += nr', g:e4)
silent! let g:e5 = v:true  | silent! let g:e5 += 1        | call Show('bool += nr', g:e5)
silent! let g:e6 = {'a':1} | silent! let g:e6 += [1]      | call Show('dict += list', g:e6)
silent! let g:e7 = 1.5     | silent! let g:e7 %= 2        | call Show('float %= nr', g:e7)
silent! let g:e8 = 1.5     | silent! let g:e8 .= 'x'      | call Show('float .= str', g:e8)
silent! let g:e9 = 'ab'    | silent! let g:e9 .= 1.5      | call Show('str .= float', g:e9)
silent! let g:e10 = 1      | silent! let g:e10 %= 1.5     | call Show('nr %= float', g:e10)
silent! let g:e11 = 1      | silent! let g:e11 += {'a':1} | call Show('nr += dict', g:e11)
silent! let g:e12 = 1      | silent! let g:e12 += function('type') | call Show('nr += funcref', g:e12)
silent! let g:e13 = 1      | silent! let g:e13 += [1]     | call Show('nr += list', g:e13)

" A failed left-hand side is reported alone: no E734 follows it.
silent! let g:nosuchvar_here += 1
call Show('undefined +=', 0)

" ---- env / register / option targets ----
let @a = 'reg'
let @a .= 'X' | call Show('@reg .=', @a)
silent! let @a += 1 | call Show('@reg +=', @a)
let $VIMLRS_LETOP = 'e'
let $VIMLRS_LETOP .= 'f' | call Show('$env .=', $VIMLRS_LETOP)
silent! let $VIMLRS_LETOP += 1 | call Show('$env +=', $VIMLRS_LETOP)
set textwidth=10
let &textwidth += 5 | call Show('&numopt +=', &textwidth)
silent! let &textwidth .= 'x' | call Show('&numopt .=', &textwidth)
set fileformat=unix
let &fileformat .= '' | call Show('&stropt .=', &fileformat)
silent! let &fileformat += 1 | call Show('&stropt +=', &fileformat)
