" R41-O1 / R40: alternation priority is a property of vim's NFA thread
" scheduler, not of "first branch wins" or "longest wins". A branch that is a
" zero-width ANCHOR loses to a longer branch at the same start, while an empty
" GROUP does not — because the anchor compiles to a state transition added to
" the CURRENT thread list, so the other branch is still running when it
" reaches NFA_MATCH at a later character.
echo matchstr('aa', '^\|a*')
echo matchstr('aa', '\<\|a*')
echo matchstr('aa', '\%(\)\|a*')
echo matchstr('aa', 'a\{0}\|a*')
echo matchstr('ab', 'a\|ab')
echo matchstr(repeat('a', 2), '^\|[a]\{}')
echo matchstr('aa', '$\|a*')
echo matchstr('aa', '\%^\|a*')
echo match('a', '\]\@!\~\{-}\|\h\{,3}')
echo matchend('a', '\]\@!\~\{-}\|\h\{,3}')
echo substitute('a', '\]\@!\~\{-}\|\h\{,3}', 'X', '')
echo substitute('a', '\]\@!\|\h\{,3}', 'X', '')
echo split('aXa', 'X\|a*')

" The same engine, on the shapes the round-1/2 corpus already pinned: a
" quantified group, a backreference, an atomic group and the lookarounds all
" still answer through it.
echo matchstr('12c3', '\(\_[a-z]\?\)[a-z]')
echo matchlist('abcabc', '\(abc\)\1')
echo matchstr('aaa', '\(a*\)\@>a')
echo matchstr('foobar', 'foo\zsbar')
echo matchstr('foobar', 'foo\@<=bar')
echo matchstr('foobar', 'foo\(bar\)\@=')
echo matchstr('foobar', 'foo\&...')
echo matchstr('abc', '\%[abc]')
echo matchstr('xay', 'a\{-1,}')
