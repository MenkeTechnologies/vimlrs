" special_none.vim — v:none rendered distinctly from v:null (eval.c specials).
" v:null and v:none are both special "empty" values, but they render differently:
" string(v:null) is 'v:null' and string(v:none) is 'v:none'. They flow through
" lists/dicts keeping their identity. Self-test into v:errors.

" --- string()/:echo render each special with its own name
call assert_equal('v:null', string(v:null))
call assert_equal('v:none', string(v:none))

" --- they keep their rendering inside containers
call assert_equal('[v:null, v:none]', string([v:null, v:none]))
call assert_equal("{'a': v:none}", string({'a': v:none}))

" --- both are the special type (type 7) and are empty()
call assert_equal(type(v:null), type(v:none))
call assert_equal(7, type(v:none))
call assert_equal(1, empty(v:none))

" --- a v:none stored in and read back from a variable still renders as v:none
let x = v:none
call assert_equal('v:none', string(x))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'special_none.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'special_none.vim: all assertions passed'
