" eval_index(): an index or slice that is read but not followed by `]` is
" E111, while a subscript whose expression itself fails keeps that failure's E15.
let l = [1, 2]
echo l[0 1]
echo l[0:1 2]
echo [1, 2][9, 8]
echo l[0
echo l[
echo l[0:
echo 'x'[0 | echo 'y'
echo 'after'
