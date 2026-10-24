" did_set_encoding(): enc_canonize() then mb_init() against vim's own
" enc_canon_table[] / enc_alias_table[] (mbyte.c), not Neovim's.
set enc=latin1
echo &enc
set enc=foo
echo &enc
set enc=utf8
echo &enc
set enc=ucs-2le
echo &enc
set enc=cp1252
echo &enc
set enc=8bit-cp1252
echo &enc
set enc=2byte-foo
echo &enc
set enc=euc-jp
echo &enc
set enc=utf-8
echo &enc
set enc=japan
echo &enc
set enc=big5
echo &enc
set enc=iso-8859
echo &enc
set enc=iso8859
echo &enc
set enc=utf-16be
echo &enc
set enc=dec-mcs
echo &enc
set enc=zh-cn
echo &enc
set enc=microsoft-cp1250
echo &enc
set enc=8bit-latin1
echo &enc
set enc=2byte-euc-jp
echo &enc
set enc=default
echo &enc
let &enc = 'bogus'
echo &enc
set enc=
echo &enc
set enc=utf-8
echo &enc
echo iconv("\xe9", 'latin1', 'utf-8')
echo iconv("\xe9", 'iso-8859', 'utf-8')
echo iconv("\xe9", 'ISO_8859-1', 'utf-8')
echo iconv("\xe9", 'dec-mcs', 'utf-8')
echo iconv('é', 'utf-8', 'utf-16be') == "\x00\xe9"
echo iconv('é', 'utf8', 'ucs2le') == "\xe9\x00"
echo iconv('a', 'zh-cn', 'utf-8')
echo iconv('a', 'japan', 'utf-8')
echo iconv("\x82\xa0", 'japan', 'utf-8')
echo iconv("\x82\xa0", 'default', 'utf-8')
