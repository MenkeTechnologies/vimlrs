" c: ex_let runs eval0() on {expr} when the command executes: text that does
" not parse is an error then (and the target is not assigned), not a reason to
" reject the file. E15 quotes the whole expression to the end of the line.
let x = 1 +
let x = = 3
let x = (1
let x = [1,
let x = 1 2
let x = ~ 3 | echo 'not run'
let x += * 2
let x = {'a' 1}
let y = 5
let y == 3
let y =~ 3
let x = 1 ~ 2
let x = 1 'abc
echo exists('x') y
let y += 1 +
echo y
echo 1 ~ 2 | echo 'not run'
echo ~ 3
echo 'end'
