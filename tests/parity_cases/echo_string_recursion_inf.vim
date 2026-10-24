" echo_string_core(): a Float is "%g" (inf, -inf, nan); :echo marks every
" container it met ([...] for a sibling too), string() only one being converted.
let inf = 1 / 0.0
let nan = 0 / 0.0
echo inf -inf nan
echo string(inf) string(-inf) string(nan)
echo [inf, -inf, nan] {'a': nan}
echo string([inf, nan])
echo inf . ''
echo printf('%s %f %g %e %d', inf, -inf, nan, inf, 1)
echo eval(string(inf)) == inf
echo json_encode(1.5)
echo str2float('inf') str2float('-inf') str2float('nan') str2float('Infinity')
let l = [1] | call add(l, l) | echo l | echo string(l)
let d = {'a': 1} | let d.b = d | echo d | echo string(d)
let x = [1] | let y = [x, x] | echo y string(y)
let l2 = [[1]] | call add(l2[0], l2) | echo l2
