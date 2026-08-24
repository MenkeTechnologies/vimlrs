" Every arm of `eval_string()` (vendor/eval.c:3560-3650) writes into a
" `char *end` — BYTES. Only `\u`/`\U` go through `utf_char2bytes`:
"
"   case 'x': case 'u': case 'U':
"     ...
"     if (c != 'X') { end += utf_char2bytes(nr, end); }
"     else          { *end++ = (char)nr; }          // \x and \X: RAW BYTE
"
"   case '0' ... '7':
"     *end = (char)(*p++ - '0');
"     if (*p >= '0' && *p <= '7') { *end = (char)((*end << 3) + (*p++ - '0')); ... }
"
" This port decoded all of them as CODE POINTS and re-encoded as UTF-8, so
" `"\xc3"` was the two bytes `c3 83` rather than the one byte `c3`, and
" `"\303\251"` was four bytes rather than the two that spell 'é'. It also
" dropped any escape whose value Rust's `char` cannot hold, and kept an
" embedded NUL that ends the string in C.

" \x / \X take at most two hex digits and store the byte itself.
echo len("\xc3") len("\XC3") len("\x41")
echo strtrans("\xff")
echo str2list("\xc3\xa9")
echo len("\x1ff") str2list("\x1ff")
" A single hex digit is fine; no hex digit at all is not an escape.
echo str2list("\x7") str2list("\x")

" Octal takes at most three digits and truncates through a char at every step,
" so 0o777 is the byte 255 and 0o400 is the byte 0.
echo len("\303\251") str2list("\303\251")
echo str2list("\777")
echo str2list("\1\2") len("\12") len("\123")

" \u / \U store the code point encoded as UTF-8 ...
echo str2list("\U0000e9") str2list("é") len("\U0001F600")
" ... with no upper range check, so this is a real four-byte sequence for a
" code point above the Unicode maximum (which is why it cannot be a Rust char).
echo str2list("\U110000")

" A byte that resolves to 0 ends the string: the buffer is NUL-terminated.
echo len("a\0b") str2list("a\0b")
echo len("\400") str2list("\u0")

" The escapes that were already bytes are unchanged.
echo str2list("\e\t\r\n\\\"") str2list("\<Esc>") str2list("\b")
" An unknown escape is the letter itself.
echo str2list("\z") len("\u") len("\xg")

" $"..." shares the same escape set, so it resolves to the same bytes.
echo len($"a\xc3b") str2list($"\xc3\xa9")
echo strtrans($"x{1+1}\xff")

" A literal that IS valid UTF-8 still takes the plain constant path.
echo len("é") str2list("é") "abc"
