" A lambda inside an evaluated expression string compiles to its own chunk; that
" chunk has to be registered or the Funcref names a body nothing defined.
echo typename(eval('{x -> x}'))
echo eval('{x -> x}')(5)
echo string(eval('{x, y -> x + y}'))
echo eval('{x, y -> x + y}')(2, 3)
echo string(map([1, 2, 3], '{a, b -> b * 10}(0, v:val)'))
echo string(filter([1, 2, 3], '{a, b -> b > 1}(0, v:val)'))
echo eval('{-> 42}')()
