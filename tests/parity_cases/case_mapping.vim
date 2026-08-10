" `toupper()`/`tolower()` and the `\U`/`\L`/`\u`/`\l` substitute escapes use
" Vim's SIMPLE (1:1) case mappings, not Unicode's full ones.
"
" Rust's `char::to_uppercase`/`to_lowercase` are the FULL mappings and can turn
" one character into several. Vim's tables hold only the 1:1 entries, so the two
" disagree wherever the full mapping expands: `toupper('ß')` was `SS` here and
" is `ß` in vim, `toupper('ﬁ')` was `FI` and is `ﬁ`, and
" `substitute('straße', '\(.*\)', '\U\1', '')` was `STRASSE` and is `STRAßE`.
"
" Taking the FIRST codepoint of the full mapping is not the fix either — that
" answers `S` for `ß` and `F` for `ﬁ`, which is not a case conversion of
" anything. The rule, checked over 2321 codepoints against vim 9.2.0900
" (U+0020..U+05FF, Latin Extended Additional, Letterlike, Alphabetic
" Presentation Forms, Fullwidth, Deseret, Cyrillic Extended-C, Latin
" Extended-C): uppercase expands => no simple mapping => leave the character
" alone; lowercase takes the first codepoint (U+0130 lowercases to `i` in vim,
" while its full mapping is `i` + U+0307).
"
" Neither engine implements the Turkish dotted/dotless-I locale rule, so the
" `i`/`I` pair below is the same answer in every locale.

echo toupper('ß') toupper('ﬁ') toupper('ﬂ') toupper('ﬀ') toupper('ŉ')
echo tolower('İ') toupper('ı') tolower('ı') toupper('I') tolower('I')
echo toupper('ä') tolower('Ä') toupper('ς') tolower('Σ') toupper('ǆ')
echo toupper('istanbul') tolower('ISTANBUL') toupper('abcXYZ') tolower('abcXYZ')
echo toupper('') tolower('') toupper('123!@#') tolower('123!@#')

echo substitute('straße', '\(.*\)', '\U\1', '')
echo substitute('STRAßE', '\(.*\)', '\L\1', '')
echo substitute('ﬁle', '\(.\)', '\u\1', '')
echo substitute('ÄBC', '\(.\)', '\l\1', '')
echo substitute('hello wörld', '\<./\=', '\u&', 'g')
echo substitute('MiXeD', '.*', '\U&\E-\L&', '')

" A round-trip over a block of codepoints, so a table change anywhere in it
" fails this case rather than only the hand-picked characters above.
let s:o = []
for s:cp in range(0x0130, 0x0180) + range(0x1e9e, 0x1eaa) + range(0xfb00, 0xfb07)
  call add(s:o, s:cp . ':' . toupper(nr2char(s:cp)) . '/' . tolower(nr2char(s:cp)))
endfor
echo join(s:o, ' ')
