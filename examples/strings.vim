" strings.vim — string builtins and the Vim-magic regex engine.
"
" Demonstrates: split()/join(), case folding, printf(), substitute() with a
" pattern, and the =~ match operator backed by the ported regex engine.
"
"   vimlrs examples/strings.vim

let s = 'The Quick Brown Fox'
echo 'upper     :' toupper(s)
echo 'lower     :' tolower(s)
echo 'words     :' split(s)
echo 'joined    :' join(split(s), '-')
echo 'length    :' strlen(s) 'bytes,' strchars(s) 'chars'
echo 'reversed  :' join(reverse(split(s)), ' ')

" printf formatting
echo printf('hex %x  pad %05d  float %.3f', 255, 42, 3.14159)

" regex: match and substitute
let line = 'error: file not found (code 404)'
echo 'matches?  :' (line =~ '\d\+')
echo 'matchstr  :' matchstr(line, '\d\+')
echo 'censored  :' substitute(line, '\d\+', '###', 'g')

" simple word-count via split
let prose = 'one two three two one one'
echo 'word count:' len(split(prose))
