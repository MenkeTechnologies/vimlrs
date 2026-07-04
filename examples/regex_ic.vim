" regex_ic.vim — case-fold (`\c` / `\C`) applies to LITERAL set members only.
" Per vim's fold rule (:help /\c): `\c` (and 'ignorecase') rewrites literal set
" members so either case matches — a literal atom (`\ca`), bracket literals
" (`[abc]`), and ranges (`[A-Z]`/`[a-z]`, negated too) all fold. But a
" case-*defined* predicate keeps its own definition: POSIX `[[:upper:]]`/
" `[[:lower:]]` and the atoms `\u`/`\l` do NOT fold, so a lowercase char must
" not match `[[:upper:]]` under `\c`. Case-agnostic predicates (`\d \w \a \x`)
" are no-ops. `\C` forces case-sensitive matching regardless of 'ignorecase'.
" Every expected value below was verified against vim 9.2 and nvim 0.12.3.
" Self-test: asserts into v:errors, throws if any failed.

" --- FOLD: literal atom `\ca` matches A too
call assert_equal('AAA', matchstr('AAAb', '\ca\+'))
" --- FOLD: bracket literals `[abc]` match ABC under \c
call assert_equal('ABC', matchstr('ABCd', '\c[abc]\+'))
" --- FOLD: range `[a-c]` matches ABC under \c
call assert_equal('ABC', matchstr('ABCd', '\c[a-c]\+'))
" --- FOLD: upper range `[A-Z]` folds to lowercase under \c
call assert_equal('abcD', matchstr('abcD', '\c[A-Z]\+'))
" --- FOLD: negated range `[^a-c]` excludes both cases under \c
call assert_equal('XYZ', matchstr('abcXYZ', '\c[^a-c]\+'))

" --- NO FOLD: POSIX [[:upper:]] stays upper-only under \c (bug: was 'ABc')
call assert_equal('AB', matchstr('ABc', '\c[[:upper:]]\+'))
" --- NO FOLD: POSIX [[:lower:]] stays lower-only under \c
call assert_equal('ab', matchstr('abC', '\c[[:lower:]]\+'))
call assert_equal(-1, match('c', '\c[[:upper:]]'))
" --- NO FOLD: atom \u stays upper-only under \c
call assert_equal('AB', matchstr('ABc', '\c\u\+'))
" --- NO FOLD: atom \l stays lower-only under \c
call assert_equal('ab', matchstr('abC', '\c\l\+'))

" --- MIXED: predicate + literal in one class — literal folds, predicate doesn't
call assert_equal('ABxY', matchstr('ABxYc', '\c[[:upper:]x]\+'))

" --- NO-OP: case-agnostic \d unaffected by \c
call assert_equal('12', matchstr('12ab', '\c\d\+'))

" --- \C forces case-sensitive: literal range does NOT fold
call assert_equal('c', matchstr('ABc', '\C[a-z]\+'))
" --- \C forces case-sensitive: bracket literals do NOT fold
call assert_equal('c', matchstr('ABc', '\C[abc]\+'))
" --- \C with a predicate atom stays sensitive as always
call assert_equal('AB', matchstr('ABc', '\C\u\+'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'regex_ic.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'regex_ic.vim: all assertions passed'
