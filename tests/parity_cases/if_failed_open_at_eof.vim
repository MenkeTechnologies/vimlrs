" A failed :if condition skips what follows to the end of the file, which
" still owes E171.
echo 'a'
if nosuch
echo 'b'
