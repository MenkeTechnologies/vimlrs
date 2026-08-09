" KNOWN OPEN (BUGS.md R22-O1): typename() of a lambda that declares NO
" parameters but captures one. vim keeps captures in the closure environment and
" out of uf_args, so it is a 0-parameter lambda and prints `func(...)`. This port
" desugars a capture into a leading parameter pre-bound by a Partial, which makes
" `{-> a}` indistinguishable from `function({x -> x}, [1])` — and vim prints
" `func(): [unknown]` for that one. Every other lambda shape is in
" typename_funcref.vim and matches.
function! Cap()
  let a = 5
  echo typename({-> a})
endfunction
call Cap()
