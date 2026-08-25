" Every variable scope, and the :execute / eval() round-trips through them.
"
" The `l:` rows are the ones that matter here: a function-local that this port
" lowers to a fusevm SLOT has no name and lives outside the scope dict, so
" reading the scope dict whole (`keys(l:)`, `string(l:)`, `get(l:, …)`) must
" disable that lowering — vim lists every local, slot or not.
"
" NOT probed here: b:changedtick, which needs the buffer model to count its own
" edits (this port's buf_T carries the field but never bumps it).

let g:G = 'global'
let b:B = 'buffer'
let w:W = 'window'
let t:T = 'tab'
let s:S = 'script'
echo 'g:G=' g:G 'b:B=' b:B 'w:W=' w:W 't:T=' t:T 's:S=' s:S
echo 'w: whole=' string(w:) ' t: whole=' string(t:)
echo 'exists:' exists('b:B') exists('w:W') exists('t:T') exists('s:S')
echo '--1'

function! Locals()
  let l:x = 1
  let y = 2
  let l:s = 'str'
  echo '  l:x=' l:x 'y=' y 'l:y=' l:y 'x=' x
  echo '  keys(l:)=' string(sort(keys(l:)))
  echo '  string(l:)=' string(l:)
  echo '  get(l:,x)=' get(l:, 'x', 'MISSING')
  echo '  has_key(l:,y)=' has_key(l:, 'y')
  echo '  outer scopes:' g:G s:S b:B w:W t:T
endfunction
call Locals()
echo '--2'

" A numeric loop over slotted locals still produces the right answer with the
" scope dict read afterwards.
function! Sum()
  let s = 0
  let i = 0
  while i < 100
    let s += i
    let i += 1
  endwhile
  echo '  sum=' s 'keys=' string(sort(keys(l:)))
endfunction
call Sum()
echo '--3'

unlet b:B
echo 'after unlet b:B=' exists('b:B')
unlet! b:nosuch
let v:errmsg = ''
silent! unlet b:nosuch2
echo 'unlet missing errmsg=' v:errmsg
let v:errmsg = ''
echo '--4'

echo 'v:true=' v:true 'v:false=' v:false 'v:null=' string(v:null) 'v:none=' string(v:none)
echo 'type(v:errmsg)=' type(v:errmsg) 'type(v:count)=' type(v:count)
silent! let v:version = 1
echo 'let v:version errmsg=' v:errmsg
let v:errmsg = ''
echo '--5'

echo 'eval round trip=' eval(string([1, {'a': 'b'}, 2.5, v:null]))
echo 'eval expr=' eval('1 + 2 * 3')
silent! echo eval('1 +')
echo 'eval bad errmsg=' v:errmsg
let v:errmsg = ''
echo 'eval of a name=' eval('g:G')
echo '--6'

echo 'execute one=' string(execute('echo 1'))
echo 'execute list=' string(execute(['echo 1', 'echo 2']))
echo 'execute silent=' string(execute('echo "x"', 'silent'))
echo 'execute nested=' string(execute('execute "echo 5"'))
echo 'eval(execute())=' string(eval('execute("echo 3")'))
let g:cmd = 'let g:made = 42'
execute g:cmd
echo 'execute built stmt=' g:made
execute 'let g:m2 =' string(7)
echo 'execute concat args=' g:m2
function! ExecLocal()
  let l:v = 0
  execute 'let l:v = 9'
  echo '  execute into l:=' l:v 'keys=' string(sort(keys(l:)))
endfunction
call ExecLocal()
echo '--7'
