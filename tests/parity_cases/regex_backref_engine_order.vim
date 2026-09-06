" `seen_endbrace()` (regexp_bt.c, called from BOTH engines) scans `regparse` —
" the pattern as the user WROTE it — for the three bytes "@<=" or "@<!", and
" accepts the backreference when it finds them. Scanning a magic TRANSLATION
" of a very-magic pattern answers differently: a real `\@<=` is written `@<=`
" there, three bytes with no backslash, and translates to `\@\<\=`.
echo split('', '\v\(\1\(\@<=')
echo substitute('', '\v,3(\2\@<=', '', 'g')
echo match('a', '\1\(\@<=')
echo match('a', '\1@<=')
echo match('a', '\1x\@<=')
echo match('a', '\v\1x\@<!')
echo match('a', '\1')
echo match('a', '\1a')
echo match('a', '\va@<=')
