" wordfreq.vim — a text-processing pipeline over a file, with embedded tests.
"
" Demonstrates the ported file-I/O and string builtins together: writefile()/
" readfile() round-trip, split() tokenizing, a dict frequency table, sort()
" with a Funcref comparator, printf() formatting, and assertions over the
" results. Exits non-zero on failure.
"
"   vimlrs examples/wordfreq.vim

" Seed a small corpus (so the example is self-contained), then read it back.
let tmp = tempname()
call writefile(['the quick brown fox', 'the lazy dog', 'the fox jumps'], tmp)
let lines = readfile(tmp)
call delete(tmp)

" Tokenize every line into one flat word list.
let words = []
for line in lines
  call extend(words, split(line))
endfor

" Build a frequency table.
let freq = {}
for w in words
  let freq[w] = get(freq, w, 0) + 1
endfor

" Sort the (word, count) pairs by descending count.
function! ByCountDesc(a, b) abort
  return a:b[1] - a:a[1]
endfunction

let pairs = []
for w in keys(freq)
  call add(pairs, [w, freq[w]])
endfor
call sort(pairs, function('ByCountDesc'))

" ── unit tests ──
call assert_equal(10, len(words))
call assert_equal(7, len(uniq(sort(copy(words)))))
call assert_equal(3, freq['the'])
call assert_equal(2, freq['fox'])
call assert_equal(1, freq['dog'])
call assert_false(has_key(freq, 'cat'))
call assert_equal(['the', 3], pairs[0])
call assert_equal(7, len(pairs))

" ── demo ──
echo 'total words :' len(words)
echo 'unique words:' len(uniq(sort(copy(words))))
echo 'most common :' pairs[0][0] '('.pairs[0][1].')'

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: wordfreq assertions passed'
