" funcrefs.vim — Funcrefs and call() over BUILTIN functions, not just user
" functions. Vim lets you call(), function() and store a funcref to any builtin
" (printf, substitute, abs, …); vimlrs resolves the name to its ported builtin.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- call() on builtins with an argument list
call assert_equal('3-4', call('printf', ['%d-%d', 3, 4]))
call assert_equal('aYb', call('substitute', ['axb', 'x', 'Y', '']))
call assert_equal(5, call('abs', [-5]))
call assert_equal(3, call('len', [[1, 2, 3]]))
call assert_equal(8, call('max', [[3, 8, 1]]))

" --- function() returns a Funcref to a builtin; invoke via a variable
let Upper = function('toupper')
call assert_equal('HELLO', Upper('hello'))

" --- a Partial binds leading args, the rest are supplied at the call site
let Censor = function('substitute', ['hello world'])
call assert_equal('hell0 w0rld', Censor('o', '0', 'g'))

" --- builtins reachable from inside map()'s string expression too
call assert_equal([1, 2, 3], map([-1, -2, -3], 'call("abs", [v:val])'))

" --- user functions still take precedence and coexist with builtins
function! Double(n)
  return a:n * 2
endfunction
call assert_equal(14, call('Double', [7]))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'funcrefs.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'funcrefs.vim: all assertions passed'
