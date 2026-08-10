" `vim_str2nr()` treats a leading MINUS as a sign and a leading PLUS as the end
" of the number (`vendor/charset.c:1228`), so every implicit String -> Number
" coercion answers 0 for `'+7'`. `str2nr()` is the exception: `f_str2nr` strips
" the sign itself before calling in, so it answers 7.
"
" This port accepted `+` inside `vim_str2nr` and therefore answered 7 (and 16,
" and 2, and 'aa') everywhere below.
echo '+7' + 0
echo '+7' * 1
echo '+7' - 0
echo '+7' == 7
echo '  +7' + 0
echo '+0x10' + 0
echo '+3.5' + 0
echo printf('%d', '+7')
echo [10, 20, 30]['+1']
echo repeat('a', '+2')
echo abs('+7')
echo '+7' . ''

" the MINUS side is unchanged, in every base and with whitespace
echo '-7' + 0
echo '-0x10' + 0
echo '  -7  ' + 0
echo '-' + 0
echo '+' + 0

" str2nr() keeps accepting both signs, in every base it takes
echo str2nr('+42') str2nr('-42') str2nr('42')
echo str2nr('+ff', 16) str2nr('-ff', 16) str2nr('+0xff', 16)
echo str2nr('+101', 2) str2nr('-101', 2) str2nr('+17', 8) str2nr('-17', 8)
echo str2nr('  +42  ') str2nr('+  42')
