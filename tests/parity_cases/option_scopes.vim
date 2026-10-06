" :set / :setlocal / :setglobal, &opt / &l:opt / &g:opt, across global,
" buffer-local, window-local and global-local options.
echo 'ul' &l:ul &g:ul &ul
set ul=7 | echo &l:ul &g:ul &ul
setlocal ul=5 | echo &l:ul &g:ul &ul
set ul=9 | echo &l:ul &g:ul &ul
setlocal ul=5 | setglobal ul=3 | echo &l:ul &g:ul &ul
let &ul = 11 | echo &l:ul &g:ul &ul
let &l:ul = 12 | echo &l:ul &g:ul &ul
set ul& | echo &l:ul &g:ul &ul
setlocal ul=4 | setlocal ul& | echo &l:ul &g:ul &ul
setlocal ul=4 | set ul< | echo &l:ul &g:ul &ul
echo 'so' &l:so &g:so &so
setlocal so=5 | echo &l:so &g:so &so
set so=2 | echo &l:so &g:so &so
setlocal so=5 | setlocal so< | echo &l:so &g:so &so
setlocal so=5 | set so< | echo &l:so &g:so &so
echo 've' &l:ve &g:ve &ve
setlocal ve=all | echo &l:ve &g:ve &ve
setlocal ve< | echo '['.&l:ve.']' &g:ve &ve
echo 'ch' &l:ch &g:ch &ch
setlocal ch=3 | echo &l:ch &g:ch &ch
echo 'ts'
setlocal ts=3 | setglobal ts=5 | echo &l:ts &g:ts &ts
set ts< | echo &l:ts &g:ts &ts
setlocal ts=3 | setlocal ts< | echo &l:ts &g:ts &ts
setlocal ts=3 | set ts& | echo &l:ts &g:ts &ts
setlocal ts=3 | setlocal ts& | echo &l:ts &g:ts &ts
setlocal ts=3 | setglobal ts& | echo &l:ts &g:ts &ts
setlocal ts+=1 | echo &l:ts &g:ts &ts
setglobal ts+=1 | echo &l:ts &g:ts &ts
setlocal ts=3 | set ts+=1 | echo &l:ts &g:ts &ts
let &g:ts = 2 | echo &l:ts &g:ts &ts
let &l:ts += 1 | echo &l:ts &g:ts &ts
let &g:ts += 1 | echo &l:ts &g:ts &ts
setlocal nu | echo &l:nu &g:nu &nu
setglobal nonu | echo &l:nu &g:nu &nu
setlocal ic | echo &l:ic &g:ic &ic
setglobal noic | echo &l:ic &g:ic &ic
let &l:ic = 1 | echo &l:ic &g:ic &ic
setlocal ft=vim | echo &l:ft '['.&g:ft.']' &ft
setlocal sw=0 sts=3 tw=7 | echo &sw &g:sw &sts &g:sts &tw &g:tw
echo getbufvar('', '&ts') getwinvar(0, '&nu')
setlocal ul=5 | setglobal ul=100 | set ul+=1 | echo &l:ul &g:ul &ul
setlocal ul=5 | setglobal ul=100 | setlocal ul+=1 | echo &l:ul &g:ul &ul
setlocal ul< | setglobal ul=100 | setlocal ul+=1 | echo &l:ul &g:ul &ul
setlocal ul< | setglobal ul=100 | setlocal ul& | echo &l:ul &g:ul &ul
setlocal so=5 | setglobal so=100 | set so+=1 | echo &l:so &g:so &so
setlocal so< | setglobal so=100 | setlocal so+=1 | echo &l:so &g:so &so
setlocal ve=all | setglobal ve=block | set ve+=onemore | echo &l:ve '/' &g:ve '/' &ve
setlocal ve= | setglobal ve=block | setlocal ve+=onemore | echo &l:ve '/' &g:ve '/' &ve
setlocal ve=all | setglobal ve=block | let &l:ve .= ',onemore' | echo &l:ve '/' &g:ve '/' &ve
setlocal ve=all | setglobal ve=block | let &ve .= ',onemore' | echo &l:ve '/' &g:ve '/' &ve
setlocal so=5 | let &so = 7 | echo &l:so &g:so &so
setlocal so=5 | let &so += 7 | echo &l:so &g:so &so
setlocal ul=5 | let &ul += 7 | echo &l:ul &g:ul &ul
setlocal ul=5 | set ul^=2 | echo &l:ul &g:ul &ul
let &ts = 'abc'
echo &ts
let &ts = '12'
echo &ts
let &ic = 'x'
echo &ic
let &ft = 1
echo &ft
let &ft = []
echo &ft
let &l:ft = 'z' | echo &ft &g:ft
let &g:ts = 4 | echo &ts &g:ts
let &t_xx = 'a'
echo getbufvar('', '&ts') getbufvar(bufnr(), '&ft')
call setbufvar('', '&ts', 6) | echo &ts &g:ts
call setwinvar(0, '&nu', 1) | echo &nu &g:nu
call setbufvar('', '&ul', 6) | echo &ul &l:ul &g:ul
echo exists('&l:ts') exists('&g:ts') exists('+ts')
