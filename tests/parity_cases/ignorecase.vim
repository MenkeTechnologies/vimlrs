set ignorecase
echo 'abc' == 'ABC'
echo 'abc' != 'ABC'
echo 'a' < 'B'
echo 'b' > 'A'
echo ['a'] == ['A']
echo {'k':'a'} == {'k':'A'}
echo index(['A'],'a')
echo 'abc' ==# 'ABC'
echo 'abc' ==? 'ABC'
set noignorecase
echo 'abc' == 'ABC'
echo 'a' < 'B'
