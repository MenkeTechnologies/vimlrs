" A VimL string is `char_u *` — bytes, with no encoding invariant — and every
" BYTE-indexed operation on one can therefore cut a multibyte character in half.
" Vim does exactly that and hands the halves back:
"
"   'ab'[1]        c: `v = xmemdupz(s + n1, 1)`   (eval.c, eval_index_inner)
"   'ab'[1:2]      c: the same branch, two bounds
"   strpart()      c: `vim_strnsave(p + nbyte, len)` — documented byte-based
"   strcharpart()  c: {start} is byte-based even though {len} counts characters
"
" The results are then rendered by the message layer, which hexes a byte that
" starts no valid UTF-8 sequence (`<c2>`), so the difference is directly
" observable — see echo_transchar.vim for that transform on its own.
"
" This case exists because all four of those call sites laundered their byte
" slice through `String::from_utf8_lossy` before storing it. That is not a
" rendering difference: it REPLACES each undecodable byte with U+FFFD (ef bf
" bd), so a one-byte subscript came back three bytes long, `len()` of it was
" wrong, and two different cuts of the same string compared EQUAL once both had
" collapsed to the same replacement character.

" `list2str` takes CODE POINTS, so this is the six bytes 41 c2 97 c3 a6 42 —
" 'A', U+0097, U+00E6, 'B'.
let s = list2str([0x41, 0x97, 0xe6, 0x42])
echo len(s) strchars(s)

" A legacy-script subscript is one BYTE, so index 1 is the lead byte c2 alone.
echo strtrans(s[1])
echo len(s[1])
" ... and index 2 is its continuation byte, which is a DIFFERENT string.
echo strtrans(s[2])
echo s[1] ==# s[2]
" Both bytes together are the character again.
echo strtrans(s[1:2])
" A negative slice bound wraps against the BYTE length, so it lands mid-character.
echo strtrans(s[-3:])
echo strtrans(s[0:0] . s[1:1] . s[2:2])

" strpart() cuts on bytes: this is 97 a5 e6, none of which starts a sequence.
echo strtrans(strpart('日本語', 1, 3))
echo len(strpart('日本語', 1, 3))
echo strtrans(strpart(s, 1, 1))
echo strtrans(strpart(s, 2, 2))

" strcharpart()'s {start} is a byte offset even though {len} counts characters.
echo strtrans(strcharpart(s, 1, 1))
echo strtrans(strcharpart(s, 0, 2))

" slice() is the character-indexed one, so it never splits anything.
echo strtrans(slice(s, 1, 2))

" The bytes survive a round trip through the list form.
echo str2list(strpart('日本語', 1, 3))
echo strtrans(list2str(str2list(s)))

" A scalar string comparison is `mb_strcmp_ic` (vendor/mbyte.c:3054), which is
" `strcmp` on BYTES when the case rule is `#`. Two different undecodable bytes
" are two different strings, and they order by byte value (c2 == 194 > 97 ==
" 151). Laundering both through `from_utf8_lossy` first collapsed them to the
" same U+FFFD, so `==#` said 1 and `>#` said 0.
echo s[1] ==# s[2]
echo s[1] !=# s[2]
echo s[1] ># s[2]
echo s[1] <# s[2]
echo s[1] is s[2]
" The container comparisons already went through tv_equal, which was byte-exact.
echo [s[1]] == [s[2]]
echo {'k': s[1]} == {'k': s[2]}
" Equal bytes still compare equal, through the same path.
echo s[1] ==# s[1]
echo sort([s[2], s[1]])->map({_, v -> strtrans(v)})

" char2nr is utf_ptr2char (vendor/eval/funcs.c:705), which returns the raw lead
" byte when the bytes at the pointer do not form a sequence.
echo char2nr(s[1]) char2nr(s[2]) char2nr(s[1:2])
echo char2nr(strpart('日本語', 1, 3))
