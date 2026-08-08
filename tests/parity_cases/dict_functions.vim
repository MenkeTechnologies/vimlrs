function! Outer()
  let n = 0
  func! s:helper() closure
    return 1
  endfunc
  return s:helper()
endfunction
echo Outer()
function! Counter()
  let c = 0
  return {-> c + 1}
endfunction
echo Counter()()
let d = {'v':7}
function d.get() dict
  return self.v
endfunction
echo d.get()
echo d['get']()
let obj = {'f': function('Counter')}
echo obj.f()()
echo 1 ? 2 : 3
echo has_key(d,'get')
echo type(function('Counter'))
const CC = 5
echo CC
echo typename(1)
lockvar d
echo islocked('d')
