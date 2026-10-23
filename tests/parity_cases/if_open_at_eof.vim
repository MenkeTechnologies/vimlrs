" A sourced file that ends inside a conditional runs its lines and then
" reports the innermost open one (c: do_cmdline, ex_docmd.c): the :while body
" runs once, as nothing reaches an :endwhile to loop back.
echo 'before'
if 1
  echo 'in the if'
  while 1
    echo 'in the loop'
