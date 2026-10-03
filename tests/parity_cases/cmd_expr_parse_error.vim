" An expression argument that does not parse is an error when the command
" RUNS (eval0 / eval_to_bool / get_func_arguments), not a reason to reject the
" file: the commands around it still run, a function holding one still exists.
function F()
  return 1 +
  echo 'after return'
endfunction
echo F()
if 1 + | echo 'a' | else | echo 'b' | endif
if 1 +
  echo 'a2'
elseif 1
  echo 'b2'
else
  echo 'c2'
endif
echo 'x1'
let i = 0
while i < 2 && 1 +
  let i += 1
endwhile
echo i
for x in [1,
  echo x
endfor
echo 'x2'
try
  throw 1 +
catch
  echo 'caught' v:exception
endtry
eval 1 +
eval [1] 2
function G()
  for x in [1,
  endfor
  return 7
endfunction
echo G()
" Not executed, so not reported.
if 0
  echo 1 +
  let x = 1 +
  call Foo(
endif
if 1 | echo 'x' | elseif 2 + | echo 'y' | endif
" A failed `:for` list is its own error only: no E1098 for a value that was
" never produced.
for x in nosuch
  echo x
endfor
for x in [1][5]
  echo x
endfor
for x in 1 . []
endfor
" get_func_arguments: anything but `,` after an argument, or running out,
" is E116. (In an expression vim also prints the source after the name, Neovim
" and this port only the name — that split is examples/parse_errors.vim.)
call Foo(
call len(1 2)
call len(1,
let l = [1, 2] | call add(l, ) | echo l
echo add([1], )
" A `,` where an argument should start is the same E116.
call assert_equal(, 1)
" `@"` is the unnamed register, never the start of a comment.
let @" = 'unnamed'
call assert_equal('unnamed', @")
echo v:errors
echo 'end'
