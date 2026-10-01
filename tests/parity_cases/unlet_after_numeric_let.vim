" A variable assigned only Numbers is still a variable :unlet can name.
let d = 5
unlet d
echo exists('d')
let e = 1
let x = 2 | unlet e
echo exists('e')
let f = 3
silent unlet f
echo exists('f')
let n = 4
unlet n | echo 'after'
for k in range(3) | let n = k | endfor
echo n
unlet n
echo exists('n')
