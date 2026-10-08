" A lambda takes `...` as its last parameter, as get_function_args allows:
" the extra arguments are in a:0 / a:000 and the named ones stay bare locals.
echo {... -> a:0}() {... -> a:0}(1, 2)
echo {... -> a:000}(1, 2)
echo {a, ... -> [a, a:000]}(1, 2, 3)
echo {a, ... -> [a, a:0]}(1)
echo call({x, ... -> x + len(a:000)}, [10, 'p', 'q'])
echo map([1, 2], {... -> a:1 * 10 + a:2})
try | echo {a, ... -> a}() | catch | echo v:exception | endtry
echo type({... -> 1}) {... -> 1}()
