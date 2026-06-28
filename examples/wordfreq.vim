" wordfreq.vim — a small text-processing pipeline over a file.
"
" Demonstrates the ported file-I/O and string builtins working together:
" writefile()/readfile() round-trip, split() tokenizing, a dict frequency
" table, sort() with a Funcref comparator, and printf() formatting.
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
echo 'total words :' len(words)
echo 'unique words:' len(uniq(sort(copy(words))))

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

echo 'frequencies (most common first):'
for p in pairs
  echo printf('  %-7s %d', p[0], p[1])
endfor
