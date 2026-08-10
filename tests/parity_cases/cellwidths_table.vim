" setcellwidths()/getcellwidths() — every rejection in `vendor/mbyte.c:2899`,
" the sort on the first codepoint, and the fact that a rejected update leaves
" the previously installed table alone. Recorded from vim 9.2.0900.
echo strwidth('☀') getcellwidths()

" a range of codepoints >= 0x80 is accepted and changes the reported width
call setcellwidths([[0x2600, 0x26ff, 2]])
echo strwidth('☀') getcellwidths()

" the empty List clears the table
call setcellwidths([])
echo strwidth('☀') getcellwidths()

" the table is reported SORTED on the first codepoint, not in input order
call setcellwidths([[0x2700, 0x2700, 2], [0x2600, 0x2600, 2]])
echo getcellwidths()

" every rejection, each leaving the installed table untouched
" (the non-List argument is NOT probed here: vim says
" `E1211: List required for argument 1` and Neovim — the porting spec, and what
" this engine follows — says `E714: List required`. See BUGS.md R31-N1.
for s:bad in [[[0x41, 0x41, 2]], [1], [[0x100]], [['a', 'b', 1]],
      \ [[0x200, 0x100, 1]], [[0x100, 0x200, 3]], [[0x100, 0x200, 0]],
      \ [[0x2600, 0x2650, 2], [0x2640, 0x2700, 1]]]
  try
    call setcellwidths(s:bad)
    echo 'accepted' string(s:bad)
  catch
    echo v:exception
  endtry
endfor
echo getcellwidths()

call setcellwidths([])
echo getcellwidths()
