" A malformed :echo / :execute argument reports when execution reaches it:
" the arguments ahead of it have already printed, the message quotes the text
" to the END OF THE LINE (vim never cut it at the |), and the rest of the line
" is abandoned.
echo 'a' 'b' *
echo 'next1'
echo 'a' 1 + * 2 | echo 'same line'
echo 'next2'
echon 'x' )
echo 'next3'
echo 'a' [*]
echo 'next4'
echo 'a' 1 +
echo 'next5'
echo 'a' (1
echo 'next6'
echo 1 + | echo 'no'
echo (1 | echo 'no'
echo 1 < < 2
echo 1 == == 2
execute 'echo' *
echo 'x' 1 ? 2
echo 1 ? 2 : *
echo {'a': 1
echo 'n7'
silent! echo 1 + *
echo 'n8'
try
  echo 'in try' 1 + *
catch
  echo 'caught' v:exception
endtry
