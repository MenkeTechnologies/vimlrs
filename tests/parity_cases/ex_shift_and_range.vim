" :> and :< build the new indent with set_indent() (tabs unless 'expandtab',
" a blank line shifted, an empty one not), 'shiftround' rounds first, and more
" than 'report' lines says so. While sourcing, an address past the last line is
" E16 with the command quoted, where interactively it would clamp.
call setline(1, ['a', '', '  c', 'd'])
%>
echo getline(1, '$') line('.') col('.')
%<<
echo getline(1, '$')
set shiftwidth=3
2,3>>>
echo getline(1, '$')
set shiftwidth=4 expandtab
1>>
echo getline(1, '$')
set noexpandtab shiftwidth=0 tabstop=4
>
echo getline(1, '$')
set shiftwidth=4 shiftround
call setline(1, ['      a', '  b'])
1,2<
echo getline(1, '$')
1,2>>
echo getline(1, '$')
call setline(1, ['foo bar', ''])
call cursor(1, 99)
echo col('.')
call cursor(2, 5)
echo [line('.'), col('.')]
9
echo line('.')
1,9d
2,9s/b/X/
$+1
echo getline(1, '$')
