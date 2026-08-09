" match()/matchend()/matchstr()/matchstrpos()/matchlist() {start} is a BYTE
" offset into the subject, not a character index: find_some_match() takes
" `len = strlen(str)` and chops with `str += start` (vendor/eval/funcs.c:4111,
" 4137, 4146). This port indexed it by character, so every {start} past the
" first multi-byte character searched from the wrong place — `match(s,'.',6)`
" was 11 where both engines say 6.
"
" EXCLUDED: a {start} that lands INSIDE a multi-byte sequence (1, 4, 7, 9, 10,
" 12 for the subject below). The C hands the regex the orphan continuation
" bytes and matches them one byte at a time — `matchstr(s,'\p',1)` is '<bc>' in
" both engines — which a char-indexed matcher cannot express. This port begins
" at the next whole character instead. Recorded as BUGS.md R26-O1.
let s = "ünïcø∂é"
" Byte offsets of the 7 characters: 0 2 3 5 6 8 11, and 13 == strlen(s).
for i in [0, 2, 3, 5, 6, 8, 11, 13, 14]
  echo i match(s, '\p', i) matchend(s, '\p', i) string(matchstr(s, '\p', i))
endfor
echo string(matchstrpos(s, '\p', 6))
echo string(matchlist(s, '\p', 11))
" {start} with {count} is a startcol, so `^` still anchors to 0.
echo match(s, '\p', 0, 2) match(s, '\p', 3, 2) match(s, '^.', 3, 1) match(s, '^.', 3)
" Negative clamps to 0; ASCII is unchanged (byte == char there).
echo match(s, '\p', -2) match('abcdef', '.', 3) match('abcdef', '.', 6) match('abcdef', '.', 7)
" List subject: {start} is an item index, not a byte offset.
echo match(['ax', 'bx', 'cx'], 'x', 1) match(['ax', 'bx', 'cx'], 'x', 1, 2)
