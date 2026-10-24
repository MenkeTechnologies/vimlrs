" did_set_encoding() for 'fileencoding', 'termencoding', 'makeencoding': enc_canonize(),
" no comma in 'fenc', a conversion with 'encoding' for 'tenc'.
echo '[' . &fenc . '][' . &tenc . '][' . &menc . ']'
set fenc=UTF8
echo &fenc &l:fenc &g:fenc
set fenc=a,b
echo &fenc
set fenc=Latin_1
echo &fenc
setglobal fenc=ISO8859_2
echo &g:fenc &l:fenc
set tenc=ISO8859_2
echo &tenc
set tenc=bogus
echo &tenc
set menc=8bit-Latin1
echo &menc &g:menc &l:menc
setlocal menc=UTF8
echo &menc &g:menc &l:menc
set menc=2byte-foo
echo &menc
let &fenc = 'Default'
echo &fenc
set fenc=
echo '[' . &fenc . ']'
echo exists('&fenc') exists('&tenc') exists('&menc')
