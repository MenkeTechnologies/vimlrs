" A command that reads an expression finds the next command itself; when the
" expression does not parse, the rest of the line is part of it: the :endif
" here is never seen, so the :if stays open (c: eval0's ends_excmd check).
if 1 | echo 'one' ) | endif
echo 'still inside the if'
if 1 | call abs(1) ) | endif
echo 'two levels deep'
while 0 ) | echo 'x' | endwhile
echo 'not reached'
