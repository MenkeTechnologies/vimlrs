" regex_look.vim — the \@ lookaround family (regexp).
" \@= / \@! assert (negatively) that the preceding atom matches AT the current
" position, zero-width; \@<= / \@<! assert it matches ENDING at the current
" position (\@123<= bounds how far back the attempt may start); \@> matches the
" atom like a standalone pattern with no backtracking into it. Every expectation
" below was verified identical in Vim 9.2 and nvim. Self-test: asserts into
" v:errors.

" --- \@! negative lookahead: zero-width wherever the atom does NOT follow
call assert_equal(['3', '.', '5', 'e', '2'], split('3.5e2', 'a\@!'))
call assert_equal('X*.[]^$\', substitute('*.[]^$\', 'a\@!', 'X', 'abc'))
call assert_equal(0, match('abc', 'b\@!'))
call assert_equal(2, match('bbc', 'b\@!'))
call assert_equal(0, match('foobaz', 'foo\(bar\)\@!'))
call assert_equal(-1, match('foobar', 'foo\(bar\)\@!'))
call assert_equal('-a-b-c-', substitute('abc', 'x\@!', '-', 'g'))

" --- \@= positive lookahead: zero-width, and groups inside it are captured
call assert_equal('foo', matchstr('foobar', 'foo\(bar\)\@='))
call assert_equal(3, matchend('foobar', 'foo\(bar\)\@='))
call assert_equal(['foo', 'bar', ''], matchlist('foobar', 'foo\(bar\)\@=')[0:2])
call assert_equal(['a', 'b'], split('ab', 'b\@='))
call assert_equal('abc', substitute('abc', 'x\@=', '-', 'g'))

" --- \@<= / \@<! lookbehind: the atom must match ending exactly here
call assert_equal('bar', matchstr('foobar', '\(foo\)\@<=bar'))
call assert_equal(-1, match('zzzbar', '\(foo\)\@<=bar'))
call assert_equal(-1, match('foobar', '\(foo\)\@<!bar'))
call assert_equal(3, match('zzzbar', '\(foo\)\@<!bar'))
call assert_equal('--b-c', substitute('aXbXc', 'X\@<!.', '-', 'g'))
call assert_equal('aXX', substitute('aaa', '\(a\)\@<=a', 'X', 'g'))

" --- the lookbehind may match empty, and the farthest start wins the captures
call assert_equal(0, match('b', '\(a*\)\@<=b'))
call assert_equal(['b', 'aaa'], matchlist('aaab', '\(a*\)\@<=b')[0:1])

" --- \@123<= limits how far back the lookbehind attempt may start
call assert_equal('', matchstr('foobar', '\(foo\)\@2<=bar'))
call assert_equal('bar', matchstr('foobar', '\(foo\)\@3<=bar'))

" --- \@> atomic group: standalone match, no backtracking into it
call assert_equal('aaab', matchstr('aaab', '\(a*\)\@>b'))
call assert_equal(-1, match('aaa', '\(a*\)\@>a'))

" --- very magic: a bare @ is the operator
call assert_equal('bar', matchstr('foobar', '\v(foo)@<=bar'))
call assert_equal(0, match('foobaz', '\vfoo(bar)@!'))
call assert_equal('bar', matchstr('foobar', '\v(foo)@3<=bar'))
call assert_equal(['3', '.', '5', 'e', '2'], split('3.5e2', '\va@!'))

" --- combined assertions at one position
call assert_equal(1, match('ab', '\(a\)\@<=\(c\)\@!b'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'regex_look.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'regex_look.vim: all assertions passed'
