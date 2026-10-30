" :redir to a variable, a register and a file: every message is recorded with
" the line break msg_start() writes before it, :silent output included, and an
" execute() running meanwhile keeps its own capture. (An error is left out: vim
" records its "Error detected while processing" preamble, which names the path.)
redir => g:out
echo 'a' 'b'
echon 'c'
silent echo 'quiet'
echo ''
let g:z = 1
let g:z
set ts? sw?
redir END
echo strtrans(g:out)

let g:acc = 'pre'
redir =>> g:acc
let r = execute('echo "inner"')
echo 'post'
redir END
echo strtrans(g:acc) strtrans(r)

function F()
  redir => out
  echo 'local'
  redir END
  return out
endfunction
echo strtrans(F()) exists('out')

redir @a
echo 'reg'
redir END
redir @A
echo 'app'
redir END
echo strtrans(@a)

let f = tempname()
execute "redir >" f
echo "to file"
redir END
execute "redir >>" f
echo "appended"
redir END
echo readfile(f)
call delete(f)

redir => 1x
redir foo
redir @ab
redir => g:x y
