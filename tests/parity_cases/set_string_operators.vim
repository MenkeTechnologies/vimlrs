" +=, ^= and -= on a string option: a comma only for a comma list, nothing
" added that is already there (P_NODUP), a doubled flag dropped (P_FLAGLIST).
set isk=@,48-57
set isk+=-
echo &isk
set isk-=48-57
echo &isk
set isk+=@
echo &isk
set isk^=-
echo &isk
set wildmode=full
set wim+=list
set wim+=list
echo &wim
set wim-=full
echo &wim
set wim-=nope
echo &wim
set ww=b,s
set ww+=h,l
echo &ww
set ww+=b
echo &ww
set brk=ab
set brk+=bc
echo &brk
set ft=vim
set ft+=x
echo &ft
set ft-=i
echo &ft
set ft^=z
echo &ft
set suffixes=.a,
set su+=.b
echo &su
set brk=\ x
echo '[' . &brk . ']'
