" :return outside a function reports E133 before reading its argument, so no
" next command is taken from the line: the :endif here is never seen.
function F(x)
  if a:x | return 1 | endif
  return 2
endfunction
echo F(1) F(0)
if 1 | return | endif
echo 'inside the open if'
