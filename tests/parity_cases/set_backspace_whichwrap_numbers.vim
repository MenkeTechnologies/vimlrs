" Vim 5.x 'backspace' numbers and Vim 3.0 'whichwrap' numbers
" (c: opt_backspace_nr2str / opt_whichwrap_nr2str): a numeric 'bs' is turned
" into its words before the next :set builds a value, a numeric 'ww' operand is
" read as its flag list.
set bs=3
echo &bs
try
  set bs=03
catch
  echo v:exception
endtry
echo &bs
set bs=1
set bs+=start
echo &bs
set bs=0
set bs-=eol
echo '[' . &bs . ']'
set bs=2
set bs^=nostop
echo &bs
for n in [0, 1, 3, 4, 8, 16, 31, 32, 7]
  exe 'set ww=' . n
  echo n '[' . &ww . ']'
endfor
set ww=31x ts=3
echo &ww &ts
set ww=b
set ww+=12
echo &ww
