" :set of a name that is not a vim option is E518, quoting the argument;
" 'w300'/'w1200'/'w9600' have no value in any build and only asking for one is
" E519 (c: do_set_option). Vim options this port does not hold are accepted.
for n in ['nosuch', 'invnosuch', 'nonosuch', 'nosuch&', 'nosuch?', 'nosuch=1',
      \ 'nosuch!', 'nosuch+=1', 'nosuch<', '&', 'x;', '1x', 'allx', 'noall',
      \ 'w300', 'w1200=4', 'w9600?', 'nonovice', 'novice', 'invnovice',
      \ 'cpo&vim', 'nocp', 'guifont=x', 'gfn=y', 'stl=x', 't_AB=x']
  try
    exe 'set ' . n
    echo n 'ok'
  catch
    echo n v:exception
  endtry
endfor
try
  set ts=1 nosuch ts=3
catch
  echo v:exception
endtry
echo &ts
for c in ['setlocal nosuch', 'setglobal nosuch']
  try
    exe c
  catch
    echo v:exception
  endtry
endfor
set nosuch
echo 'after'
