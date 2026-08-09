" A subscript only attaches to the expression it abuts. handle_subscript()
" (vendor/eval.c:5961) loops while
"
"   ((**arg == '[' || (**arg == '.' && rettv->v_type == VAR_DICT)
"     || (**arg == '(' && (!evaluate || tv_is_func(*rettv))))
"    && !ascii_iswhite(*(*arg - 1)))
"   || (**arg == '-' && (*arg)[1] == '>')
"
" so `l[0]` indexes but `l [0]` is TWO `:echo` arguments — the List and a
" one-item List literal. `->` is the single form the C exempts from the
" whitespace guard, which is why `x ->len()` still calls the method.
let l = [1, 2, 3]
let d = {'a': 1}
let s = 'abc'

" Abutting: index, slice, dict member, chained.
echo l[0]
echo l[0:1]
echo l[-1]
echo d['a']
echo s[1]
echo [[1, 2], [3, 4]][1][0]

" Spaced: a separate argument, never a subscript. The rendering of the second
" argument is the tell — `[0]` prints as a List, not as an element.
echo l [0]
echo d ['a']
echo s [1]
echo 12345 [1, 2]
echo 1.5 [1, 2]
echo (1) [1, 2]
echo l [0] [1]

" Same rule inside an expression, not just in an argument list: with a space the
" `[` starts a new List, so this is List + List concatenation.
echo l + [0]
echo string(l) . string([0])

" `->` is exempt: whitespace before it does not stop the method call.
echo l ->len()
echo l->len()
echo [1, 2] ->add(3)

" `(` follows the same guard as `[`: a spaced one is a separate argument.
echo function('strlen') ('ab')
echo function('strlen')('ab')
