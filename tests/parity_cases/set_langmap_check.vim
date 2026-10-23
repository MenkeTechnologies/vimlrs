" 'langmap' values are checked pair by pair (c: did_set_langmap, map.c):
" E357 for a character without its match, E358 for text after the second half.
for v in ['abc', 'ab;cd', 'aAbB', 'abc;ABC', 'ab;A', 'ab;ABC', 'abc;AB',
      \ 'ab;AB,cd', 'ab;ABx', 'a\,b', '\;a', 'aA,bB,', ',aA', 'éÉ', 'é', 'ab;CD,',
      \ 'a\', 'aA;', ';', ';ab', 'a', "a\x01", 'ab;CD,xy;XY,zZ', 'ab;CD,xy', 'ä;',
      \ 'a;ä']
  try
    let &langmap = v
    echo v 'ok' &lmap
  catch
    echo v v:exception
  endtry
endfor
set lmap=xy;
