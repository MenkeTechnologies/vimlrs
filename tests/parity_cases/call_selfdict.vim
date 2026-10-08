" call()'s {dict} argument is `self` for the call. A partial that read its
" dict automatically (d.Fn) yields to it; one bound explicitly
" (function(F, d)) keeps its own (call_func).
let D = {'n': 1}
function! D.Get() dict
  return self.n
endfunction
function! DictFn() dict
  return get(self, 'n', 'none')
endfunction
function! NoDict()
  return exists('self')
endfunction
echo call('DictFn', [], {'n': 10})
echo call(function('DictFn'), [], {'n': 11})
echo call(D.Get, [], {'n': 12})
echo call(function(D.Get, D), [], {'n': 13})
echo call(function('DictFn', {'n': 14}), [], {'n': 15})
echo call(function('DictFn', [], {'n': 16}), [], {'n': 17})
echo call('NoDict', [], {'n': 1})
try | echo call('DictFn', []) | catch | echo v:exception | endtry
let E = {'n': 20, 'Get': D.Get}
echo E.Get() call(E.Get, []) call(E.Get, [], D)
echo call({-> self}, [], {'x': 1})
