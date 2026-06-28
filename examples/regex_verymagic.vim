" regex_verymagic.vim — \v "very magic" regex mode (regexp).
" After \v, every ASCII punctuation char is an operator without a backslash:
" ( ) group, | alternation, + ? * {n,m} quantifiers, < > word bounds. A
" backslash makes such a char literal instead. \v applies to the rest of the
" pattern. Note: regex patterns use single quotes so '\v' is not a string
" escape. Self-test into v:errors.

" --- quantifiers and groups without backslashes
call assert_equal('123', matchstr('abc123', '\v\d+'))
call assert_equal('color', matchstr('color', '\vcolou?r'))
call assert_equal('colour', matchstr('colour', '\vcolou?r'))
call assert_equal('aa', matchstr('aaa', '\va{2}'))

" --- alternation and capture groups
call assert_equal('foo', matchstr('foobar', '\v(foo|baz)'))
call assert_equal(['ab', 'a', 'b'], matchlist('ab', '\v(a)(b)')[0:2])

" --- anchors and word boundaries
call assert_equal(1, 'hello_1' =~ '\v^\w+$')
call assert_equal('cat', matchstr('a cat here', '\v<cat>'))

" --- \v with substitute(), and a backslash makes an operator literal
call assert_equal('aXbX', substitute('a1b2', '\v\d', 'X', 'g'))
call assert_equal('(b)', matchstr('a(b)c', '\v\(b\)'))

" --- magic mode (the default) still works unchanged
call assert_equal('123', matchstr('abc123', '\d\+'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'regex_verymagic.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'regex_verymagic.vim: all assertions passed'
