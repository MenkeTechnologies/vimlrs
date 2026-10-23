" A string option's did_set_* callback (optionstr.c) rejects a malformed value
" and the old value stays: E474 for most, E539 for a 'whichwrap' flag, E21 for
" 'fileformat' in a nomodifiable buffer. :let &opt reports the bare message.
let g:cases = [
      \ 'bs=foo', 'bs=4', 'bs=03', 'bs=indent,foo', 'bs+=foo',
      \ 'ff=foo', 'ffs=x', 'ffs=mac,', 've=foo', 've=all,foo', 'sel=foo', 'sel=',
      \ 'mps=ab', 'mps=a:b:c', 'mps=a:bc', 'mps+=x', 'ww=z', 'ww=b,X',
      \ 'ambw=x', 'ambw=', 'wim=x', 'wim=lon', 'wim=full,full,full,full,full',
      \ 'wim=:full', 'wim=full::list', 'cb=x', 'cb=unnamed,', 'km=x', 'km+=foo',
      \ 'isk=300', 'isk=a,', 'isk=^a,b-a', 'isk=0', 'isk=a-b-c', 'isk=@-@,^,,',
      \ 'isk=a-', 'isk=a\ ,b', 'isk=255,256', 'isp=1-300', 'isf=x,,y',
      \ 'spf=foo.txt', 'spf=.add', 'spf=a.add,',
      \ 'spf=a:b.add', 'ft=foo/bar', 'ft=x\ y', 'ft=x;y', 'syn=x*y', 'syn=a@b',
      \ 'bt=foo', 'cb=exclude:\\(']
for c in g:cases
  let name = matchstr(c, '^\a\+')
  let before = eval('&' . name)
  try
    exe 'set ' . c
    echo c 'accepted:' eval('&' . name)
  catch
    echo v:exception
  endtry
  if eval('&' . name) !=# before
    echo '  changed:' eval('&' . name)
  endif
  exe 'let &' . name . ' = before'
endfor
for c in ['bs=2', 'bs=', 'bs=eol,start,eol', 've=block,insert,all,onemore,none,NONE',
      \ 'sel=old', 'km=startsel,stopsel', 'km=startsel,', 'wim=longest:full,list:lastused',
      \ 'wim=full,', 'cb=unnamed,unnamedplus,autoselect,html', 'cb=exclude:x',
      \ 'cb=html,exclude:x,unnamed', 'ff=mac', 'ffs=', 'ffs=mac,dos,unix', 'ambw=double',
      \ 'mps=<:>,a:b', 'mps=<:>,', 'mps=é:ü', 'ww=<,>,~,[,]', 'ww=,,,', 'isk=^', 'isk=^a',
      \ 'isk=@,^@', 'isk=-', 'isk=é', 'isi=,', 'ft=FOO.bar-1', 'ft=',
      \ 'spf=foo.add,bar.add', 'spf=x\ y.add', 'bt=nofile', 'bt=']
  exe 'set ' . c
  echo c '->' eval('&' . matchstr(c, '^\a\+'))
endfor
set isf=é
try
  set spf=a.add
catch
  echo 'isfname follows the option:' v:exception
endtry
set isf&
set spf=a.add
echo &spf
try
  let &ww = "b,\x01"
catch
  echo v:exception
endtry
try
  let &l:ve = 'foo'
catch
  echo v:exception
endtry
set noma
for c in ['set ff=dos', 'setlocal ff=mac', 'let &ff = "mac"', 'setglobal ff=dos']
  try
    exe c
    echo c 'ok'
  catch
    echo v:exception
  endtry
endfor
echo &ff
set ma
" A rejected 'spelllang' never reaches the spell/{lang}.vim sourcing.
for c in ['spl=a/b', 'spl=en/x', 'spl=en\ x', 'spl=*']
  try
    exe 'set ' . c
  catch
    echo v:exception
  endtry
endfor
echo &spl
