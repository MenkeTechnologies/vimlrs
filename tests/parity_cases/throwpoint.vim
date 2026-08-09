" v:throwpoint — the location an exception was raised at.
"
" The raw value embeds the absolute path of this file, so it can never be
" compared verbatim between two machines. Two things are recorded instead:
" whether a point was captured at all, and everything from the file's basename
" onwards — which is the whole exception stack (frame chain, per-frame entry
" lines, and the raising line) minus the directory.
"
" That tail also drops the one part of vim's chain this interpreter cannot
" reproduce: vim was launched with `-c 'source …'`, so its chain begins
" `command line..script …`; vimlrs is handed the script path directly and has no
" such entry. Everything after it is byte-identical, which is what `tail()`
" compares.
function! Tail()
  return substitute(v:throwpoint, '.*[/\\]', '', '')
endfunction

try
  throw 'x'
catch
  echo 'direct' (v:throwpoint != '')
  echo 'direct' Tail()
endtry

function! Thrower()
  throw 'inner'
endfunction
try
  call Thrower()
catch
  echo 'in-function' (v:throwpoint != '')
  echo 'in-function' Tail()
endtry

try
  echo undefined_zzz
catch
  echo 'from-error' (v:throwpoint != '')
  echo 'from-error' Tail()
endtry

" Two frames deep: each entry carries the line IT was at when it called the
" next, and only the innermost gets the `, line N` suffix.
function! Outer()
  call Thrower()
endfunction
try
  call Outer()
catch
  echo 'nested' Tail()
endtry

" A throw from the third line of a body reports 3 — function bodies are numbered
" from 1 at the line after the `:function`, not by file line.
function! Deep()
  let x = 1
  let y = 2
  throw 'deep'
endfunction
try
  call Deep()
catch
  echo 'body-line' Tail()
endtry

" An error inside a loop reports the loop body's line, not the `:for`.
try
  for i in [1, 2]
    echo [][0]
  endfor
catch
  echo 'in-loop' Tail()
endtry

" `:endtry` restores the value the enclosing level had, exactly as it does for
" v:exception.
try
  throw 'outer'
catch
  try
    throw 'inner'
  catch
  endtry
  echo 'restored' Tail()
endtry
echo 'after' (v:throwpoint == '')
