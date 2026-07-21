" funcref_get.vim — get() on a Funcref/Partial reads its introspection fields
" (funcs.c f_get, the tv_is_func branch): "name", "func", "dict", "args" and
" "arity". Values pinned against vim 9.2 and nvim 0.12 (they agree).
"
" Control-flow quirks that this pins (all match the C source):
"   • "name"/"func"/"args"/"arity" ignore the {def} 3rd argument entirely.
"   • "dict" is special: it sets the result to the bound dict, but then still
"     lets a present {def} OVERWRITE it — so get(P, 'dict', X) is always X, and
"     get(F_no_dict, 'dict') with no default is 0.
"   • an unknown {what} raises E475 (not a silent default).
"   • "arity" folds already-bound partial args into required/optional.
"
"   vimlrs examples/funcref_get.vim

function! MyF(a, b, ...) abort
  return 0
endfunction

function! MyG(a, b = 5) abort
  return 0
endfunction

" ── name: the resolved function name, as a String ──
call assert_equal('add', get(function('add'), 'name'))
call assert_equal('MyF', get(function('MyF'), 'name'))
call assert_equal('add', get(function('add', [[9]]), 'name'))
" {def} is ignored when the field exists
call assert_equal('add', get(function('add'), 'name', 'DEFLT'))

" ── func: the same name, but typed as a Funcref ──
call assert_equal(function('add'), get(function('add'), 'func'))
call assert_equal(type(function('add')), type(get(function('add'), 'func')))

" ── args: the List of bound leading arguments (empty List, never 0) ──
call assert_equal([], get(function('add'), 'args'))
call assert_equal([[9]], get(function('add', [[9]]), 'args'))
call assert_equal([1, 2], get(function('add', [1, 2], {'x': 1}), 'args'))
call assert_equal([1], get(function('MyF', [1]), 'args'))

" ── dict: the bound self dict, or the {def}/0 fallthrough ──
call assert_equal({'x': 1}, get(function('add', {'x': 1}), 'dict'))
call assert_equal(0, get(function('add'), 'dict'))
" the fallthrough: a present {def} overwrites even a bound dict
call assert_equal('DEFLT', get(function('add', {'x': 1}), 'dict', 'DEFLT'))
call assert_equal('DEFLT', get(function('add'), 'dict', 'DEFLT'))

" ── arity: {required, optional, varargs}, adjusted by bound args ──
call assert_equal({'required': 2, 'optional': 0, 'varargs': v:false}, get(function('add'), 'arity'))
call assert_equal({'required': 2, 'optional': 0, 'varargs': v:true}, get(function('MyF'), 'arity'))
call assert_equal({'required': 0, 'optional': 0, 'varargs': v:true}, get(function('MyF', [1, 2]), 'arity'))
call assert_equal({'required': 1, 'optional': 1, 'varargs': v:false}, get(function('MyG'), 'arity'))
" a builtin's arity, and binding args that consume the required slots
call assert_equal({'required': 4, 'optional': 0, 'varargs': v:false}, get(function('substitute'), 'arity'))
call assert_equal({'required': 2, 'optional': 0, 'varargs': v:false}, get(function('substitute', ['a', 'b']), 'arity'))
call assert_equal({'required': 0, 'optional': 0, 'varargs': v:false}, get(function('substitute', ['a', 'b', 'c', 'd']), 'arity'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'funcref_get.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'funcref_get.vim: all assertions passed'
