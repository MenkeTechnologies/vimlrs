" strmanip.vim — string/list building: substitute/tr/repeat/split/join
" (strings.c/mbyte.c). substitute() with a LITERAL pattern (no metacharacters)
" replaces the first ('' flag) or every ('g' flag) occurrence; tr() maps each
" character in 'from' to the same-position character in 'to'; repeat()
" concatenates a string N times or tiles a list; split() breaks on a literal
" separator (with an optional keepempty flag), join() is its inverse.
" Self-test: asserts into v:errors, throws if any failed.

" --- substitute(): literal-letter patterns only (no regex metacharacters)
call assert_equal('fXXbar', substitute('foobar', 'o', 'X', 'g'))
call assert_equal('fXobar', substitute('foobar', 'o', 'X', ''))
call assert_equal('bbbbbb', substitute('aaa', 'a', 'bb', 'g'))
call assert_equal('abc', substitute('abc', 'z', 'Y', 'g'))
call assert_equal('cafe', substitute('café', 'é', 'e', 'g'))
call assert_equal('heLLo', substitute('hello', 'l', 'L', 'g'))

" --- tr(): position-wise character mapping between two equal-length sets
call assert_equal('hippo', tr('hello', 'el', 'ip'))
call assert_equal('xyzxyz', tr('abcabc', 'abc', 'xyz'))
call assert_equal('ABC', tr('abc', 'abc', 'ABC'))
call assert_equal('hello', tr('hello', 'xyz', 'abc'))

" --- repeat(): tile a string, or a list, N times (N<=0 gives empty)
call assert_equal('ababab', repeat('ab', 3))
call assert_equal('', repeat('x', 0))
call assert_equal('==========', repeat('=', 10))
call assert_equal([1, 2, 1, 2, 1, 2], repeat([1, 2], 3))
call assert_equal([0, 0, 0, 0], repeat([0], 4))
call assert_equal([], repeat([1], 0))

" --- split(): literal separator; default drops empties, flag 1 keeps them
call assert_equal(['a', 'b', 'c'], split('a,b,c', ','))
call assert_equal(['one', 'two', 'three'], split('one two three', ' '))
call assert_equal(['a', '', 'b'], split('a,,b', ','))
call assert_equal(['a', 'b'], split(':a:b:', ':'))
call assert_equal(['', 'a', 'b', ''], split(':a:b:', ':', 1))
call assert_equal([], split('', ','))
call assert_equal(['a', 'b', 'c'], split('a::b::c', '::'))

" --- join(): inverse of split with a chosen glue (default single space)
call assert_equal('a-b-c', join(['a', 'b', 'c'], '-'))
call assert_equal('123', join([1, 2, 3], ''))
call assert_equal('x y', join(['x', 'y']))
call assert_equal('', join([], ','))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'strmanip.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'strmanip.vim: all assertions passed'
