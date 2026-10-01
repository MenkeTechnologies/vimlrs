" eval_list(): running out after an item with no comma is E696, running out
" after a comma (or before any item) is E697; a silent item failure leaves the
" caller's E15 over the whole argument.
echo [1,
echo 'a'
echo [1
echo 'b'
echo [1 2]
echo 'c'
echo [
echo 'd'
echo [1, 
echo 'e'
echo [1 +
echo 'f'
echo eval('[1,')
