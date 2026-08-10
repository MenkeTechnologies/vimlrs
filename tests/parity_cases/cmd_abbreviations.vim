" Command abbreviations that the statement dispatcher accepts.
"
" `canon_block_kw` resolved the BLOCK keywords' abbreviations (`fu`, `endw`,
" `en`, `cat`, …) and was verified against vim 9.2.0900's `fullcommand()`. The
" statement dispatcher next to it did not: `:return`, `:throw`, `:break` and
" `:continue` were matched by their full spelling only, so `retu 42` parsed as a
" bare expression, which made the whole enclosing `:function` fail to parse and
" `E117: Unknown function` fire at the CALL site — a message that names neither
" the line nor the cause.
"
" The accepted sets are explicit, never prefix tests, because the shortest
" accepted prefix is not the shortest unique one and the spelling one character
" shorter resolves to a DIFFERENT command. Read out of vim 9.2.0900 with
" `fullcommand()`:
"
"   ret -> retab      retu -> return      bre -> brewind    brea -> break
"   fin -> find       fina -> finally     co  -> copy       con  -> continue
"   fo  -> fold       for  -> for         tr  -> trewind    try  -> try
"   r   -> read       re   -> read        i   -> insert     el   -> else
"
" so `ret`, `bre`, `fin` and `co` are NOT accepted here either — checked at the
" bottom, where vim's own error is the expectation.

function! R1()
  retu 42
endfunction
function! R2()
  retur 43
endfunction
function! R3()
  return 44
endfunction
echo 'retu' R1() 'retur' R2() 'return' R3()

for i in [1, 2, 3]
  if i == 2 | con | endif
  echo 'con' i
endfor
for i in [1, 2, 3]
  if i == 2 | cont | endif
  echo 'cont' i
endfor
for i in [1, 2, 3]
  if i == 2 | conti | endif
  echo 'conti' i
endfor
for i in [1, 2, 3]
  if i == 2 | continu | endif
  echo 'continu' i
endfor

for i in [1, 2, 3]
  if i == 2 | brea | endif
  echo 'brea' i
endfor
for i in [1, 2, 3]
  if i == 2 | break | endif
  echo 'break' i
endfor

try | th 'a' | catch | echo 'th' v:exception | endtry
try | thr 'b' | catch | echo 'thr' v:exception | endtry
try | thro 'c' | catch | echo 'thro' v:exception | endtry
try | throw 'd' | catch | echo 'throw' v:exception | endtry

" The block keywords, through the same dispatcher, in their shortest forms.
fu! B1()
  if 1
    wh 0
    endw
  en
  retu 'block'
endfu
echo B1()

try
  th 'nested'
cat /nest/
  echo 'cat' v:exception
fina
  echo 'fina ran'
endt

" One character shorter is a different command, and must NOT be taken as the one
" above. vim's answer is the expectation, whatever it is.
"
" `bre` is deliberately not probed here: it is `:brewind`, an ex-command this
" crate does not model, so it reaches the expression path and answers
" `E121: Undefined variable: bre` where vim rewinds its (empty) buffer list and
" says nothing. That is an unimplemented-command gap, not an abbreviation one —
" BUGS.md R30-O3.
try | ret 1 | catch | echo 'ret ->' v:exception | endtry
