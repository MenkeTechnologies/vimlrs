echo printf('%d', 1.5) . '|'
echo printf('a%db', [1]) . '|'
echo printf('%s=%d', 'x', 3)
let r = printf('%x', {})
echo string(r)
silent! let q = printf('%d', 2.5)
echo string(q)
