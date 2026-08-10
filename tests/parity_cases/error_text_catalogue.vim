" E<number> texts that were hardcoded WRONG in this port, each pinned to what
" vim 9.2.0900 emits. Every probe is inline `try`/`catch` so the recorded line
" is the message and its throwpoint, not the abort behaviour.
"
" Two members of the audited set are deliberately absent because vim and the
" Neovim porting spec disagree on them and this engine follows Neovim; they are
" covered by `examples/sysinfo.vim` and BUGS.md R31-N1 instead:
"   * `setcellwidths(1)`  — vim E1211 / Neovim E714
"   * `let n[0:1] = …` on a Number — vim "E689: Index not allowed after a
"     number: …" / Neovim "E689: Can only index a List, Dictionary or Blob"

" E979 carries the offending index.
let s:b = 0z0011
try | let s:b[7] = 0xff | catch | echo v:exception | endtry
echo s:b

" E709 is the wrong-VALUE error of a `[:]` assignment (the wrong-BASE error is
" E689, and this port used to answer E709 for both).
let s:l = [1,2,3]
try | let s:l[0:1] = 'ab' | catch | echo v:exception | endtry
echo s:l

" E799 spells out the constraint it enforces.
try | echo matchadd('Search', 'x', 10, -3) | catch | echo v:exception | endtry
try | echo matchaddpos('Search', [1], 10, 0) | catch | echo v:exception | endtry

" E1211 names the argument position.
try | echo matchstrlist('x', 'p') | catch | echo v:exception | endtry
try | echo matchstrlist(1, 'p') | catch | echo v:exception | endtry
