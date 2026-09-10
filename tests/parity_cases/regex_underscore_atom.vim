" `\_x` is legal only for classchars, ^, $ and [ — anything else is E877.
try | echo string(matchstr('abc', '\_b')) | catch | echo v:exception | endtry
try | echo string(matchstr('abc', '\_z')) | catch | echo v:exception | endtry
try | echo string(matchstr('abc', '\_1')) | catch | echo v:exception | endtry
try | echo string(matchstr('abc', '\_(a\)')) | catch | echo v:exception | endtry
try | echo string(matchstr('abc', '\_é')) | catch | echo v:exception | endtry
try | echo string(matchstr('abc', '\_')) | catch | echo v:exception | endtry
echo string(matchstr("a\nb", '\_.\+'))
echo string(matchstr("a\nb", '\_s'))
echo string(matchstr("a\nb", '\_[ab]\+'))
echo string(matchstr('abc', '\_W'))
echo string(matchstr('abc', '\_^a'))
echo string(matchstr('abc', 'c\_$'))
echo string(matchstr('abc', 'a\_^b'))
echo string(split("a\nb", '\_s'))
