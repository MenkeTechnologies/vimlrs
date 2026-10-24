" vim parses an expression over the whole line, so a List or Dict cut short by
" a | reads its next item from the bar: E15 over the rest, or E696/E722 quoting
" it.
echo [1, | echo 3
echo [1 | echo 4
echo {'a': 1, | echo 5
echo {'a': 1 | echo 6
echo [ | echo 7
echo (1 | echo 8
echo [1,
echo 'n'
