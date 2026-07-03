" typeintro.vim — value introspection across every type: type/empty/len
" (eval.c/typval.c). type() returns the numeric type tag (0 Number, 1 String,
" 2 Funcref, 3 List, 4 Dict, 5 Float, 6 Bool, 7 Special/null, 10 Blob).
" empty() is true for 0, '', [], {}, 0.0, v:false and v:null; len() is the byte
" count for a String, element count for List/Dict/Blob, digit count for a Number.
" Self-test: asserts into v:errors, throws if any failed.

" --- type(): one tag per value kind
call assert_equal(0, type(0))
call assert_equal(1, type('x'))
call assert_equal(2, type(function('tr')))
call assert_equal(3, type([]))
call assert_equal(4, type({}))
call assert_equal(5, type(0.0))
call assert_equal(6, type(v:true))
call assert_equal(6, type(v:false))
call assert_equal(7, type(v:null))
call assert_equal(10, type(0z00))

" --- empty(): the "nothing here" predicate for every type
call assert_equal(1, empty(0))
call assert_equal(0, empty(1))
call assert_equal(1, empty(''))
call assert_equal(0, empty('a'))
call assert_equal(1, empty([]))
call assert_equal(0, empty([0]))
call assert_equal(1, empty({}))
call assert_equal(0, empty({'a': 1}))
call assert_equal(1, empty(0.0))
call assert_equal(1, empty(v:false))
call assert_equal(0, empty(v:true))
call assert_equal(1, empty(v:null))
call assert_equal(1, empty(0z))
call assert_equal(0, empty(0z00))
call assert_equal(0, empty(function('tr')))

" --- len(): bytes for String, element count for containers, digits for Number
call assert_equal(1, len(0))
call assert_equal(5, len(12345))
call assert_equal(5, len('hello'))
call assert_equal(6, len('héllo'))
call assert_equal(3, len([1, 2, 3]))
call assert_equal(0, len([]))
call assert_equal(2, len({'a': 1, 'b': 2}))
call assert_equal(0, len({}))
call assert_equal(3, len(0z001122))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'typeintro.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'typeintro.vim: all assertions passed'
