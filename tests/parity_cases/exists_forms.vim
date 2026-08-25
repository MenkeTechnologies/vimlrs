" exists() — every form f_exists dispatches on (vendor/eval/funcs.c:1363-1400).
"
" The last of them is var_exists (vendor/eval/vars.c:3371), which is NOT a plain
" name lookup: it reads the leading name and then applies whatever `d.key` /
" `l[idx]` / `f(expr)` subscripts follow (c:3386-3397), swallowing any error
" raised on the way — so `exists('g:d.nokey')` is 0 and leaves v:errmsg alone.
"
" NOT probed here: `exists(':cmd')`, which needs vim's full ex-command name
" table (this port has only the abbreviations its dispatcher accepts).

let v:errmsg = ''

echo 'undefined       =' exists('g:nosuch')
let g:yes = 1
echo 'defined         =' exists('g:yes')
echo 'bare name       =' exists('yes')
unlet g:yes
echo 'after unlet     =' exists('g:yes')

echo 'builtin func    =' exists('*strlen')
echo 'missing func    =' exists('*nosuchfunction')
function! MyFunc()
endfunction
echo 'user func       =' exists('*MyFunc')
echo 'user func g:    =' exists('*g:MyFunc')
delfunction MyFunc
echo 'after delfunc   =' exists('*MyFunc')

echo 'option short    =' exists('&tw')
echo 'option long     =' exists('&textwidth')
echo 'option missing  =' exists('&nosuchoption')
echo 'option +form    =' exists('+textwidth')
echo 'option garbage  =' exists('&tw junk')

echo 'env set         =' exists('$PATH')
echo 'env unset       =' exists('$VIMLRS_NO_SUCH_ENV_VAR')

echo 'v: known        =' exists('v:version')
echo 'v: unknown      =' exists('v:nosuchvimvar')

echo 'empty string    =' exists('')

" Subscripted names, the var_exists half.
let g:l = [1, 2]
let g:d = {'a': 1, 'nested': {'k': 2}}
echo 'list item       =' exists('g:l[0]')
echo 'list item last  =' exists('g:l[1]')
echo 'list item oob   =' exists('g:l[9]')
echo 'list item neg   =' exists('g:l[-1]')
echo 'dict key        =' exists('g:d.a')
echo 'dict key miss   =' exists('g:d.nokey')
echo 'dict subscript  =' exists("g:d['a']")
echo 'dict nested     =' exists('g:d.nested.k')
echo 'dict nested no  =' exists('g:d.nested.no')
echo 'subscript on nr =' exists('g:l[0][0]')
echo 'trailing junk   =' exists('g:l junk')
echo 'unknown base    =' exists('g:nosuch[0]')

echo 'errmsg untouched=[' . v:errmsg . ']'

" a: and l: scopes inside a function.
function! Scopes(a, ...)
  let l:loc = 1
  echo '  a:a           =' exists('a:a')
  echo '  a:0           =' exists('a:0')
  echo '  a:1           =' exists('a:1')
  echo '  a:nosuch      =' exists('a:nosuch')
  echo '  l:loc         =' exists('l:loc')
  echo '  l:nosuch      =' exists('l:nosuch')
  echo '  a:000[0]      =' exists('a:000[0]')
  echo '  a:000[9]      =' exists('a:000[9]')
endfunction
call Scopes(1, 2)
