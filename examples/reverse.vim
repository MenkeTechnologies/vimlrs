" reverse.vim — reverse() over all three reversible types (list.c + strings.c).
" reverse() accepts a List, a Blob, or a String. Lists and Blobs are reversed
" in place and the same object is returned; a String yields a new String,
" reversed by character (each base character keeps its trailing composing
" marks). Anything else returns 0. Self-test: asserts into v:errors.

" --- List: reversed in place
let l = [1, 2, 3, 4]
call assert_equal([4, 3, 2, 1], reverse(l))
call assert_equal([4, 3, 2, 1], l)

" --- Blob: bytes reversed in place
let b = 0z01020304
call assert_equal(0z04030201, reverse(b))
call assert_equal(0z04030201, b)

" --- String: a new reversed String (ASCII and multibyte by character)
call assert_equal('cba', reverse('abc'))
call assert_equal('olléh', reverse('héllo'))
call assert_equal('', reverse(''))

" --- the String form is non-destructive: the source is unchanged
let s = 'abc'
call assert_equal('cba', reverse(s))
call assert_equal('abc', s)

" --- a double reverse is the identity
call assert_equal('a man a plan', reverse(reverse('a man a plan')))
call assert_equal([1, 2, 3], reverse(reverse([1, 2, 3])))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'reverse.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'reverse.vim: all assertions passed'
