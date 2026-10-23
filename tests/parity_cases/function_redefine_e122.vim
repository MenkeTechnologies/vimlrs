" `:function` without `!` on an existing name is E122 and keeps the old body;
" `function!` replaces it. The same through an expanded name.
function Twice()
  return 1
endfunction
function Twice()
  return 2
endfunction
echo Twice()
function! Twice()
  return 3
endfunction
echo Twice()
let fname = 'Twice'
function {fname}()
  return 4
endfunction
echo Twice()
function! {fname}()
  return 5
endfunction
echo Twice()
if !exists('*Guarded')
  function Guarded()
    return 'first'
  endfunction
endif
if !exists('*Guarded')
  function Guarded()
    return 'second'
  endfunction
endif
echo Guarded()
