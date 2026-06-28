" strings.vim — string builtins and the Vim-magic regex engine, with tests.
"
" Demonstrates: split()/join(), case folding, printf(), substitute(), the =~
" match operator and matchstr(), plus assert_match() for pattern assertions.
" Exits non-zero on failure.
"
"   vimlrs examples/strings.vim

let s = 'The Quick Brown Fox'
let line = 'error: file not found (code 404)'

" ── unit tests ──
call assert_equal('THE QUICK BROWN FOX', toupper(s))
call assert_equal('the quick brown fox', tolower(s))
call assert_equal(['The', 'Quick', 'Brown', 'Fox'], split(s))
call assert_equal('The-Quick-Brown-Fox', join(split(s), '-'))
call assert_equal(19, strlen(s))
call assert_equal('Fox Brown Quick The', join(reverse(split(s)), ' '))
call assert_equal('hex ff  pad 00042  float 3.142', printf('hex %x  pad %05d  float %.3f', 255, 42, 3.14159))
call assert_true(line =~ '\d\+')
call assert_equal('404', matchstr(line, '\d\+'))
call assert_equal('error: file not found (code ###)', substitute(line, '\d\+', '###', 'g'))
call assert_match('^error:', line)
call assert_notmatch('^\d', line)

" ── demo ──
echo 'upper    :' toupper(s)
echo 'words    :' split(s)
echo 'matchstr :' matchstr(line, '\d\+')
echo 'censored :' substitute(line, '\d\+', '###', 'g')

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: strings assertions passed'
