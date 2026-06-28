" funcref_call.vim — calling a funcref-valued expression directly with (args).
" A Funcref obtained from any expression — function('name'), a lambda literal, a
" list/dict element, or a variable — can be called by writing (args) right after
" it. The '(' must abut the expression (no space), so 'a' (x) stays two values.
" Self-test into v:errors.

" --- call the result of function() directly
call assert_equal('HI', function('toupper')('hi'))
call assert_equal('f00bar', function('substitute')('foobar', 'o', '0', 'g'))

" --- call a lambda literal directly
call assert_equal(42, {x -> x * 2}(21))
call assert_equal(7, {a, b -> a + b}(3, 4))

" --- call a funcref stored in a list / dict element
let fns = [function('toupper'), function('tolower')]
call assert_equal('HIhi', fns[0]('Hi') . fns[1]('Hi'))
let d = {'up': function('toupper')}
call assert_equal('YO', d['up']('yo'))

" --- a funcref in a variable still calls the normal way
let F = function('toupper')
call assert_equal('OK', F('ok'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'funcref_call.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'funcref_call.vim: all assertions passed'
