" A regex scan advances one whole CHARACTER at a time — `MB_PTR_ADV` /
" `utfc_ptr2len`, a base codepoint plus its composing marks — so a match can
" never begin on a mark that belongs to the character before it. The three scan
" loops in src/viml_regex.rs stepped one codepoint, so `\W` matched the acute of
" a decomposed `é` and `substitute()` replaced it.
"
" R6-6 already made a matching ATOM consume the whole cluster; this is the other
" half, the position the scan starts from.
let a = nr2char(0x65) . nr2char(0x301) . "x"
echo strlen(a) strchars(a)
echo match(a, '\W') matchend(a, '\W') string(matchstr(a, '\W')) string(matchlist(a, '\W'))
echo match(a, '\w') matchend(a, '\w') string(matchstr(a, '\w'))
echo string(substitute(a, '\W', '!', 'g'))
echo string(substitute(a, '.', '<&>', 'g'))
echo string(split(a, '\zs'))
" A subject that OPENS with a composing mark still matches it at 0: the scan
" always tries its starting position before advancing.
let b = nr2char(0x301) . "z"
echo match(b, '\W') string(matchstr(b, '\W')) string(substitute(b, '\W', '!', 'g'))
" {count} advances one character past the match START, marks included.
let c = nr2char(0x65) . nr2char(0x301) . nr2char(0x65) . nr2char(0x301)
echo match(c, '\w', 0, 1) match(c, '\w', 0, 2) match(c, '\w', 0, 3)
