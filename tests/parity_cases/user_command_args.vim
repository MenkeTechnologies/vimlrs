" User commands: -count takes a leading number from the argument, -nargs=0 with
" an argument is E488, -nargs=1/+ without one E471, a ! without -bang E477 (do_one_cmd).
command! -nargs=* Foo echo [<f-args>]
Foo a b\ c
command! -nargs=1 Bar echo <q-args>
Bar x "y"
command! -bang -count=3 Baz echo <bang>0 <count>
Baz! 5
Baz
command -nargs=? Q echo '<args>'
Q
delcommand Q
Q
command! -range Rng echo <line1> <line2>
Rng
function! s:F(...)
  return a:000
endfunction
command! -nargs=+ Call echo s:F(<f-args>)
Call 1 2
echo exists(':Foo') exists(':Nope') exists(':echo')
command! -complete=file -nargs=1 Comp echo <q-args>
Comp ok
com Abc echo 'abc'
Ab
command! -nargs=0 Z echo 'z'
Z 1
command! -nargs=1 One echo 'one' <q-args>
One
command! -nargs=+ Plus echo 'plus' <q-args>
Plus
command! -count=3 Cnt echo 'cnt' <count> <q-args>
Cnt 0
Cnt 7
Cnt 7x
Cnt
command! -count Cnt2 echo 'cnt2' <count>
Cnt2
Cnt2 4
command! -nargs=* -count=2 CntA echo 'cnta' <count> '<args>'
CntA 3 foo
CntA foo 3
command! -nargs=0 Z echo 'z'
Z "comment
command! -bar -nargs=0 Zb echo 'zb'
Zb | echo 'after2'
echo 'end'
command! Nb echo 'nb'
Nb!
command! -bang -nargs=1 B echo <q-args>
command! -count=1 C echo <count>
C 99999999999999999999
C 012
