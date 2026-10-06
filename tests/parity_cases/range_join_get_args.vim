" range() argument checks (E726/E727/E1510, an empty List on failure), the same
" checks for a native for-in-range loop, join()/has_key()/get() argument checks.
echo join([1, 2], 3)
echo join([1, 2], [])
echo string(join(v:null))
echo has_key(v:null, 'a')
echo string(range('x'))
echo string(range(1, 'y'))
echo string(range(0, 9223372036854775807))
echo string(range(0, 10, 9999999999))
echo string(range(5, 4)) string(range(5, 3))
echo string(range(-9223372036854775807, 9223372036854775807, 2))
echo string(get(v:null, 1, 'd')) string(get('x', 0))
echo 'end'
for i in range(3, 1)
echo i
endfor
for i in range(2, 1)
echo i
endfor
for i in range(0, 4, 0)
echo i
endfor
for i in range(0, 9999999999)
echo i
endfor
for i in range(1, 'y')
echo i
endfor
let n = 3
for i in range(n, 1)
echo i
endfor
echo 'e'
function! F()
for i in range(3, 1)
echo i
endfor
for i in range(2, 1)
echo i
endfor
for i in range(0, 4, 0)
echo i
endfor
for i in range(0, 9999999999)
echo i
endfor
for i in range(1, 'y')
echo i
endfor
let n = 3
for i in range(n, 1)
echo i
endfor
echo 'e'
endfunction
call F()
echo join([1, [2], {}], ',')
echo join('abc')
echo get([1], -1) get([1], 'x') get('abc', 1)
echo string(get(v:null, 1, 'd'))
