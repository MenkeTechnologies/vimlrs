" :return/:break/:continue out of a :try run its :finally first, and leave the
" try level behind them: an error after the call is an error again.
function A()
  try
    return 1
  catch
  endtry
endfunction
echo A()
echo [][0]
echo 'next'
for i in range(3)
  try
    if i == 1 | break | endif
    echo 'body' i
  finally
    echo 'fin' i
  endtry
endfor
for i in range(3)
  try
    if i == 1 | continue | endif
    echo 'body' i
  finally
    echo 'fin' i
  endtry
endfor
function B()
  try
    try
      return 'b'
    finally
      echo 'inner fin'
    endtry
  finally
    echo 'outer fin'
  endtry
endfunction
echo B()
function C()
  for i in range(3)
    try
      return i
    finally
      echo 'c fin' i
    endtry
  endfor
endfunction
echo C()
function D()
  try
    throw 'x'
  catch
    return 'from catch'
  finally
    echo 'd fin'
  endtry
endfunction
echo D()
function E()
  try
    return 'e1'
  finally
    return 'e2'
  endtry
endfunction
echo E()
while 1
  try
    for j in [1, 2]
      try
        break
      finally
        echo 'inner loop fin' j
      endtry
    endfor
    break
  finally
    echo 'while fin'
  endtry
endwhile
echo 'done'
