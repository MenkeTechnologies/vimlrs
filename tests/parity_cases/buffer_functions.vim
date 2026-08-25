" The buf*() family and the scoped-variable getters/setters.
"
" c: they all resolve their first argument through tv_get_buf()
" (vendor/eval/funcs.c:471) or find_buffer() (vendor/eval/buffer.c:47) — a
" Number is a buffer number, "" is the current buffer, "$" is the last one, and
" a name is matched against the buffer list. f_bufnr (buffer.c:425) deliberately
" does NOT use tv_get_buf_from_arg so a bad argument reports one error, not two.
"
" vim always has a current buffer: main.c creates the initial unnamed one with
" buflist_new(NULL, NULL, 1, BLN_CURBUF | BLN_LISTED), which is why every answer
" below is about buffer 1 and not about "no buffers".

echo 'bufnr()      =' bufnr()
echo "bufnr('%')   =" bufnr('%')
echo "bufnr('')    =" bufnr('')
echo "bufnr('\$')   =" bufnr('$')
echo 'bufnr(1)     =' bufnr(1)
echo 'bufnr(9)     =' bufnr(9)
echo "bufnr('nosuchname') =" bufnr('nosuchname')
echo '--1'

echo 'bufname()    =[' . bufname() . ']'
echo "bufname('%') =[" . bufname('%') . ']'
echo 'bufname(1)   =[' . bufname(1) . ']'
echo 'bufname(9)   =[' . bufname(9) . ']'
echo '--2'

echo 'bufexists(1) =' bufexists(1) 'bufexists(9)=' bufexists(9)
echo "bufexists('')=" bufexists('')
echo 'buflisted(1) =' buflisted(1) 'buflisted(9)=' buflisted(9)
echo 'bufloaded(1) =' bufloaded(1) 'bufloaded(9)=' bufloaded(9)
echo '--3'

echo 'bufwinnr(1)  =' bufwinnr(1) 'bufwinnr(9)=' bufwinnr(9)
echo 'bufwinid(1)  =' bufwinid(1) 'bufwinid(9)=' bufwinid(9)
echo 'winnr()      =' winnr() "winnr('\$')=" winnr('$')
echo 'tabpagenr()  =' tabpagenr() 'tabpagewinnr(1)=' tabpagewinnr(1)
echo 'winbufnr(1)  =' winbufnr(1) 'winbufnr(9)=' winbufnr(9)
echo 'winlayout()  =' string(winlayout())
echo '--4'

" getbufvar(): an absent name yields {def}; "" yields the whole scope Dict, which
" is why b:changedtick shows up in it.
let b:one = 7
echo 'getbufvar one    =' string(getbufvar(1, 'one', 'DEF'))
echo 'getbufvar absent =' string(getbufvar(1, 'nosuch', 'DEF'))
echo 'getbufvar nodef  =' string(getbufvar(1, 'nosuch'))
echo 'getbufvar tick   =' string(getbufvar(1, 'changedtick', 'DEF'))
echo 'getbufvar all t  =' type(getbufvar(1, ''))
echo 'getbufvar badbuf =' string(getbufvar(9, 'one', 'DEF'))
echo '--5'

call setbufvar(1, 'two', 8)
echo 'setbufvar        =' string(b:two) string(getbufvar(1, 'two', 'DEF'))
call setbufvar(9, 'three', 9)
echo 'setbufvar badbuf =' exists('b:three')
echo '--6'

" The window and tab-page pairs are the same shape.
let w:wv = 'W'
let t:tv = 'T'
echo 'getwinvar        =' string(getwinvar(1, 'wv', 'DEF')) string(getwinvar(1, 'nosuch', 'DEF'))
echo 'gettabvar        =' string(gettabvar(1, 'tv', 'DEF')) string(gettabvar(1, 'nosuch', 'DEF'))
echo 'gettabwinvar     =' string(gettabwinvar(1, 1, 'wv', 'DEF'))
call setwinvar(1, 'wv2', 'W2')
call settabvar(1, 'tv2', 'T2')
call settabwinvar(1, 1, 'wv3', 'W3')
echo 'setwinvar        =' string(w:wv2)
echo 'settabvar        =' string(t:tv2)
echo 'settabwinvar     =' string(w:wv3)
echo '--7'

" A `&name` argument reads/writes the OPTION rather than a variable.
set textwidth=11
echo 'getbufvar &tw    =' string(getbufvar(1, '&textwidth', 'DEF'))
call setbufvar(1, '&textwidth', 22)
echo 'setbufvar &tw    =' &textwidth
echo '--8'
