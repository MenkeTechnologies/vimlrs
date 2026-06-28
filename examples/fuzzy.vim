" fuzzy.vim — matchfuzzy()/matchfuzzypos(), the faithful port of Neovim's
" search.c fuzzy matcher (sequential/camel/separator/first-letter bonuses).
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- matchfuzzy() keeps only items the pattern fuzzy-matches, best score first
call assert_equal(['help', 'hello', 'shell'], matchfuzzy(['hello', 'world', 'help', 'shell'], 'hl'))

" --- a non-subsequence pattern drops the item entirely
call assert_equal([], matchfuzzy(['world'], 'xz'))

" --- an exact prefix outscores a scattered match (clipboard vs cardboard)
call assert_equal(['clip', 'cardiologist'], matchfuzzy(['cardiologist', 'clip'], 'cli'))

" --- matchfuzzypos() returns [items, positions, scores]; positions are the
"     0-based char indices that matched, scores are search.c's integer scores
call assert_equal([['help', 'hello'], [[0, 2], [0, 2]], [113, 112]], matchfuzzypos(['hello', 'help'], 'hl'))

" --- separator bonuses (matches after a space) outweigh a plain contiguous run
let res = matchfuzzypos(['xhelloy', 'h e l l o'], 'hello')
call assert_equal(['h e l l o', 'xhelloy'], res[0])
call assert_equal([0, 2, 4, 6, 8], res[1][0])
call assert_equal([1, 2, 3, 4, 5], res[1][1])

" --- by default a space splits the pattern into independently-matched words,
"     so 'f b' matches 'foobar' (f…b); 'matchseq' matches it as one sequence
"     (space included), which 'foobar' lacks
call assert_equal(['foobar'], matchfuzzy(['foobar'], 'f b'))
call assert_equal([], matchfuzzy(['foobar'], 'f b', {'matchseq': 1}))

" --- the 'key' option fuzzy-matches a Dict field instead of the item itself
let people = [{'name': 'Alice'}, {'name': 'Bob'}, {'name': 'Albert'}]
call assert_equal([{'name': 'Alice'}, {'name': 'Albert'}], matchfuzzy(people, 'al', {'key': 'name'}))

" --- the 'limit' option caps the number of returned matches
call assert_equal(['ab', 'axb'], matchfuzzy(['ab', 'axb', 'axxb'], 'ab', {'limit': 2}))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'fuzzy.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'fuzzy.vim: all assertions passed'
