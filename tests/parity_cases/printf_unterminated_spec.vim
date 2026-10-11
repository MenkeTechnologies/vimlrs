" A format that ends inside a conversion spec contributes nothing: the C
" default case takes `str_arg_l = 0` when `*p` is NUL, so the `%`, flags, width,
" precision and length modifier all vanish.
for f in ['%', 'a%', '%-5', '%5.', '%l', '%ll', '%h', '%#', 'x%+', '%%', '%5%']
  echo string(f) '->' string(printf(f))
endfor
