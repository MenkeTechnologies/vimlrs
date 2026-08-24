" `eval_number()` reads an integer with `vim_str2nr(..., strict = TRUE, ...)`,
" and the strict flag has one job (vendor/charset.c:1366):
"
"   // Check for an alphanumeric character immediately following, that is
"   // most likely a typo.
"   if (strict && ptr - start != maxlen && ASCII_ISALNUM(*ptr)) {
"     return;                    // leaves *len == 0
"   }
"
" A zero length is `semsg(_(e_invalid_expression_str), *arg)` + FAIL, so a
" number with a letter or digit glued to it is NOT "a number followed by a
" name" — it is a parse failure AT the number. This port read it the other way
" and answered E121 for every case below.
"
" Which text the E15 carries is the second half of the contract, and it is not
" the same in every position: `eval_number` reports from the literal onwards,
" but only `if (evaluate)`. In a branch evaluation never reaches, nothing is
" reported there and `eval0`'s own fallback fires instead — over the WHOLE
" expression, because that is where eval0 started.

" Evaluated: the message starts at the literal and runs to the end of the
" expression (`*arg`, not the failing character).
echo 12abc
echo 1e5
echo 3x
echo 007a
" `0x`/`0b` with no digit of that radix is not a radix literal: vim_str2nr
" reads decimal `0` and then trips over the letter.
echo 0xg
echo 0b2
" A float's grammar is `[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?` and a trailing letter
" rejects it wholesale, so what is left is `1` `.` `5e3x` — and the `5` is the
" literal that fails.
echo 1.5x
echo 1.5e3x
" No `.` means no float at all, so `1e-10` is `1` with `e-10` glued to it.
echo 1e-10
" `_` is not alphanumeric, so it does NOT trip the check: the literal still
" ends at it and what follows is an ordinary name, which makes `1_000` two
" `:echo` arguments rather than one failure.
let _000 = 'name'
let _3 = 'name3'
echo 1_000
echo 12_3

" Nested in a list, still evaluated: from the literal to the end of the source.
echo [1, 12abc]
echo 1e5 . 'x'

" Not evaluated — the whole expression, from eval0's `arg`.
echo 0 ? 12abc : 3
echo 0 && 12abc
echo 1 || 12abc
" ... and evaluated again, for contrast, in the same shape.
echo 1 ? 12abc : 3

" The same two positions for the re-split-float junk that shares this machinery
" (`1.0e300` after a `.` concat is `1` `.` `0e300`).
echo 'a' . 1.0e300
echo 0 ? 'a' . 1.0e300 : 3
echo 0 && ('a' . 1.0e300)

" Everything that was already a valid literal stays one.
echo 0x10 017 0b11 0o17 1.2.3 1.0e5
echo 9223372036854775807
echo 0z0011
