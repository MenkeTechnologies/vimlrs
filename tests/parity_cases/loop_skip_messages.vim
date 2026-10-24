" A loop that does not run, or has finished, reads its body once in skip mode,
" where a block command with nothing to close and :return still report.
while 0 | endif | endwhile
echo 'a'
for i in [] | endtry | endfor
echo 'b'
for i in range(0) | endif | endfor
echo 'c'
echo 'd'
while 0
  return
endwhile
echo 'e'
echo 'f'
let k = 0
while k < 2
  let k += 1
  echo 'k' k
endwhile
while 0
  while 1
    else
  endwhile
endwhile
echo 'g'
