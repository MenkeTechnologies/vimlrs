" :let @r reads its value with tv_get_string_chk, detects the type from a
" trailing NL, and E354s on a register that cannot be written. @/ and @= are
" stored; @= reads back its source, getreg('=') evaluates it.
let @/ = 'pat'
echo @/ getreg('/') getregtype('/')
call setreg('/', 'xx')
echo @/
let @a = ['l1', 'l2']
echo getreg('a', 1, 1)
let @a = {}
let @a = "x\n"
echo getregtype('a')
let @a = 'x'
let @a .= "y\n"
echo getregtype('a') string(getreg('a'))
let @: = 'x'
let @. = 'x'
let @% = 'x'
echo setreg('?', 'x')
let @= = '1+1'
echo @= getreg('=') getreg('=', 1)
let @= .= '+1'
echo @= getreg('=')
let @= = 'nosuch'
echo getreg('=') 'after'
