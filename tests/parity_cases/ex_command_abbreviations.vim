" Every prefix of a command from its documented minimum up to the full name
" is that command (c: find_ex_command scans cmdnames[] in table order).
let d = 5
let g:acc = []
ech d
ech'quoted'
echoms 'echomsg' d
cal add(g:acc, 'cal')
ev add(g:acc, 'ev')
eva add(g:acc, 'eva')
echo g:acc
let u = 1
unle u
echo exists('u')
let w = [1]
lockv w
unloc w
unlock w
call add(w, 2)
echo w
for i in range(3)
  if i == 1
    contin
  endif
  echo i
endfor
echon 'echon' | echo ''
let s = 'x'
comma Abbrev echo 'user command'
Abbrev
delco Abbrev
echo exists(':Abbrev')
