" string_index.vim — character indexing and slicing of strings (eval.c).
"
" Mirrors eval_index_inner() (eval.c:3237): str[i] is a single character, and
" str[a:b] is an *inclusive* range (unlike slice()'s exclusive end), including
" the open-ended str[a:]. Self-tests with assert_*; exits non-zero on any
" failure.
"
"   vimlrs examples/string_index.vim

" ── str[i]: one character; out-of-range is the empty string, never an error ──
call assert_equal('h', 'hello'[0])
call assert_equal('o', 'hello'[4])
call assert_equal('', 'hello'[9])
" A *negative* subscript does NOT count from the end — it is out of range, and
" the result is the empty string. (eval.c:3296: "If the index is too big or
" negative the result is empty.") Only a slice bound counts from the end; see
" below. Vim 9.2 and Neovim 0.12 both return '' here.
call assert_equal('', 'hello'[-1])
call assert_equal('', 'hello'[-5])

" ── str[a:b]: inclusive end; contrast slice()'s exclusive end ──
call assert_equal('ell', 'hello'[1:3])
call assert_equal('llo', 'hello'[2:])
call assert_equal('llo', 'hello'[-3:-1])
call assert_equal('bc', slice('abcdef', 1, 3))
call assert_equal('bcd', 'abcdef'[1:3])
" a reversed range is the empty string
call assert_equal('', 'abc'[2:1])

" ── multibyte: str[i] is a *byte* subscript, matching Vim (BUGS.md #8, fixed
" round 17) ──
" 'héllo'[1] is the first byte of the 2-byte 'é' (eval.c:3300,
" `xmemdupz(s + n1, 1)`). Vim carries the raw lead byte 0xc3; vimlrs stores
" strings as UTF-8 text, so the split character surfaces as U+FFFD — the same
" thing Vim's byte renders as once lossily decoded.
let word = 'héllo'
call assert_equal(5, strchars(word))
call assert_equal(nr2char(0xFFFD), word[1])
call assert_equal('hé', strcharpart(word, 0, 2))
call assert_equal(char2nr('é'), strgetchar(word, 1))
" strgetchar past the end returns -1
call assert_equal(-1, strgetchar('ab', 5))

" ── strpart() works in bytes, strcharpart() in characters ──
call assert_equal('ell', strpart('hello', 1, 3))
call assert_equal('llo', strpart('hello', 2))

" ── demo ──
echo "'héllo'[1]      ->" word[1]
echo "'hello'[1:3]    ->" 'hello'[1:3]
echo "'hello'[-3:-1]  ->" 'hello'[-3:-1]

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'string_index.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
