" separate_nextcmd() drops the backslash before a '|' in a :set argument, and
" :set then removes its own escapes (c: stropt_copy_value).
set cb=exclude:a\|b
echo &cb
set cb=exclude:a\\|b
echo &cb
set cb=exclude:a\\\|b
echo &cb
set cb=exclude:a\\\\|b
echo &cb
exe 'set cb=exclude:c\\|d'
echo &cb
set cb=exclude:e\|f | echo 'second command'
