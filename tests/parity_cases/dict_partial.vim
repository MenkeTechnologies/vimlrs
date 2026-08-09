" Reading a function out of a Dict binds that Dict as its `self` — vim's
" `set_selfdict` -> `make_partial`. Three things are pinned here, each of which
" was wrong before:
"
"   1. `string()` of a bound partial shows the dict: `function('1', {…})`.
"   2. The binding only happens for a function declared `dict` (or defined with
"      the `:function d.key()` form, which implies it). A plain function stored
"      in a Dict stays a plain Funcref.
"   3. A dict written by the script (`function('F', d)`) is NOT re-bound when the
"      value is later read out of another Dict; an auto-bound one IS.
"
" It also pins where the binding does NOT happen: `get(d, 'key')` and a Funcref
" inside a List come back unbound, and the Dict's own `string()` shows the
" stored value, not the bound one.
function! NoDict(a)
  return a:a
endfunction
function! WithDict(a) dict
  return a:a + self.n
endfunction

let d = {'n': 7}
let d.plain = function('NoDict')
let d.bound = function('WithDict')
echo 'plain   ' string(d.plain)
echo 'bound   ' string(d.bound)
echo 'index   ' string(d['bound'])
echo 'get     ' string(get(d, 'bound'))
echo 'inlist  ' string([function('WithDict')][0])
echo 'dict    ' string(d)

" Bound on read, so the reference carries `self` wherever it goes.
let B = d.bound
echo 'read    ' string(B)
echo 'call    ' B(1)

" Re-binding: an auto-bound reference follows the Dict it is read from.
let e = {'n': 90}
let e.bound = d.bound
echo 'rebind  ' string(e.bound)
echo 'recall  ' e.bound(1)

" An explicitly-bound partial keeps the dict the script gave it.
let f = {'n': 500}
let f.fixed = function('WithDict', [2], d)
echo 'fixed   ' string(f.fixed)
echo 'fixcall ' f.fixed()

" `:function d.key()` implies `dict` even with no attribute written.
let g = {'n': 3}
function g.implied()
  return self.n
endfunction
echo 'implied ' string(g.implied)
echo 'impcall ' g.implied()
