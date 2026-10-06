" iconv() — port of f_iconv()/convert_setup()/string_convert() (vendor/mbyte.c):
" names are canonized (aliases, `8bit-`, iso8859N, `_`), latin1/latin9 <->
" UTF-8 is converted internally (an unrepresentable character is 0xbf, a wide
" one 0xbf + `?`, an illegal byte fails to ''), every other pair goes through
" iconv(3) with `?` for what cannot be converted. It used to return the text
" unchanged for all but a latin1 subset.
echo iconv('é', 'utf-8', 'latin1') == "\xe9" len(iconv('é', 'utf-8', 'latin1'))
echo iconv("\xe9", 'latin1', 'utf-8') iconv("\xe9", 'LATIN_1', 'UTF8')
echo iconv("\xa4", 'latin9', 'utf-8') iconv('€', 'utf-8', 'iso-8859-15') == "\xa4"
echo iconv('€x', 'utf-8', 'latin1') == "\xbfx"
echo iconv('中', 'utf-8', 'latin1') == "\xbf?"
echo iconv('é', 'utf-8', 'ascii') iconv('中a', 'utf-8', 'ascii')
echo iconv('x', 'nosuch', 'utf-8') iconv('abc', 'utf-8', 'utf-8')
echo iconv("\xff", 'utf-8', 'latin1') == ''
echo iconv("\xe9t\xe9", 'cp1252', 'utf-8')
echo iconv('Привет', 'utf-8', 'koi8-r') == "\xf0\xd2\xc9\xd7\xc5\xd4"
echo iconv("\xf0\xd2\xc9\xd7\xc5\xd4", 'koi8-r', 'utf-8')
echo iconv('é', 'utf-8', 'utf-16le') == "\xe9"
echo iconv('', 'utf-8', 'latin1') == ''
echo iconv('é', '8bit-latin1', 'utf-8') iconv('é', 'utf-8', 'iso8859-1') == "\xe9"
echo iconv('ü', 'default', 'latin1') == "\xfc"
