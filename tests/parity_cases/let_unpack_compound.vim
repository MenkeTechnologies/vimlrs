" ex_let_vars hands the operator to every ex_let_one: `let [a, b] += list`
" applies it target by target, and `; rest` gets the remaining items as a List.
let [a, b] = [1, 2]
let [a, b] += [10, 20] | echo a b
let [a, b] -= [1, 1] | echo a b
let [a, b] *= [2, 3] | echo a b
let [a, b] /= [4, 7] | echo a b
let [a, b] %= [3, 2] | echo a b
let [s, t] = ['x', 'y'] | let [s, t] .= ['1', '2'] | echo s t
let [s, t] ..= ['a', 'b'] | echo s t
let [a; r] = [1, 2] | let [a; r] += [5, [3]] | echo a r
let l = [1, 2] | let d = {'k': 'v'}
let [l[0], d.k] += [100, 'w'] | echo l d
try | let [a, b] += [1] | catch | echo v:exception | endtry
try | let [a, b] += [1, 2, 3] | catch | echo v:exception | endtry
echo a b
try | let [a, nosuch] += [1, 2] | catch | echo v:exception | endtry
echo a exists('nosuch')
try | let [s, a] += ['1', 2] | catch | echo v:exception | endtry
echo s a
let q = [1] | lockvar q
try | let [a, q] += [1, [2]] | catch | echo v:exception | endtry
echo a q
