" The a: scope is filled in C order (a:0, a:000, a:firstline, a:lastline, named
" args, then a:1..), and a scope read copies the hashtab as it is.
function! F(a, ...)
  return a:
endfunction
echo F(1, 2)
function! G(a, b)
  return a:
endfunction
echo G(1, 2)
function! H(x, y, z, ...)
  return a:
endfunction
echo H(1, 2, 3, 4, 5, 6)
function! D(a, b = a:0 + a:firstline)
  return a:b
endfunction
echo D(1)
" A lambda's named arguments live in l:, not a:.
echo {a, b -> a:}(1, 2)
echo {a -> l:}(1)
echo {a -> exists('a:a') . exists('l:a')}(1)
echo {... -> a:}(1, 2)
