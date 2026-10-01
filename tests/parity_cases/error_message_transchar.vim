" An error message is displayed through msg_outtrans(): a control byte in it
" shows as ^X (TAB ^I, NL ^@), while :echo writes TAB raw.
echo eval("1\x01")
echo eval("1\t")
echo eval("1\n2")
echo eval("1\r")
echo eval("1 \x1b")
echo strlen([1]) "a\tb"
echoerr "x\ty\nz"
echo execute("echo eval(\"1\\t\")")
let g:r = execute("echo eval(\"1\\t\")", "silent!")
echo g:r
try
 echo eval("1\t")
catch
 echo v:exception
endtry
