" :set opt? and a bare number/string option show the value (c: showoneopt):
" in silent Ex mode the value is buffered on stdout until :set ends, while the
" line breaks around it go out at once, so values a failing :set showed wait
" for the next display or for the exit.
echo 'a'
set ts?
echo 'b'
set ts? sw?
echo 'c'
set ic? noic?
echon 'd'
set ts sw
echon 'e'
set bs? ww? sel?
set wc? report? mfd?
set enc ff? ffs
setlocal sw?
setglobal ts?
echo 'f'
set ts!
echo 'g'
set et?
set ts nosuch
echo 'h'
set lines? co?
set isk? mps
set ul nosuch
echo 'last'
