" :execute from a function hands do_cmdline the function's getline, so a
" :return in its text returns from the function — at the end of the line.
function F()
  exe 'return 7'
  return 1
endfunction
echo F()
function G()
  for i in range(3)
    exe 'if i == 1 | continue | endif'
    echo i
    if i == 2 | exe 'break' | endif
  endfor
endfunction
call G()
function H(x)
  exe "if a:x | exe 'return ' . a:x * 2 | endif"
  return -1
endfunction
echo H(4) H(0)
function I()
  try
    exe 'return "t"'
  finally
    echo 'fin'
  endtry
endfunction
echo I()
function K()
  exe 'return' | echo 'rest of the line'
endfunction
echo K()
function L()
  exe 'echo "x" | return 9 | echo "y"'
endfunction
echo L()
