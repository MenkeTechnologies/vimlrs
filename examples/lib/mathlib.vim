" mathlib.vim — a small library file, loaded by sourcing.vim via :source.
function! Square(n) abort
  return a:n * a:n
endfunction

function! Cube(n) abort
  return a:n * a:n * a:n
endfunction

let g:mathlib_loaded = 1
