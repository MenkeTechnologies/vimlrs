" history.vim — histadd()/histget()/histnr()/histdel(), the command-line
" history rings ported from Neovim's cmdhist.c.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- a fresh history is empty
call assert_equal(0, histnr('cmd'))
call assert_equal('', histget('cmd'))

" --- histadd() appends; histnr() counts; histget() reads the newest
call assert_equal(1, histadd('cmd', 'ls'))
call assert_equal(1, histadd('cmd', 'pwd'))
call assert_equal(2, histnr('cmd'))
call assert_equal('pwd', histget('cmd'))

" --- adding an existing entry de-duplicates (moves it to newest)
call assert_equal(1, histadd('cmd', 'ls'))
call assert_equal(2, histnr('cmd'))
call assert_equal('ls', histget('cmd'))

" --- positive index is the 1-based absolute entry number, negative counts back
call assert_equal('pwd', histget('cmd', 1))
call assert_equal('ls', histget('cmd', 2))
call assert_equal('ls', histget('cmd', -1))
call assert_equal('pwd', histget('cmd', -2))
call assert_equal('', histget('cmd', 99))

" --- the named histories are independent
call assert_equal(1, histadd('search', 'foo'))
call assert_equal(1, histnr('search'))
call assert_equal(2, histnr('cmd'))

" --- an empty item or an invalid history name fails (returns 0 / -1)
call assert_equal(0, histadd('cmd', ''))
call assert_equal(-1, histnr('nosuch'))

" --- histdel() with a Number removes that indexed entry
call assert_equal(1, histdel('cmd', 1))
call assert_equal(1, histnr('cmd'))
call assert_equal('ls', histget('cmd'))

" --- histdel() with no item clears the whole ring
call assert_equal(1, histdel('cmd'))
call assert_equal(0, histnr('cmd'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'history.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'history.vim: all assertions passed'
