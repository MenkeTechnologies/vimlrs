echo 'abc' ==? 'ABC'
echo 'abc' ==# 'ABC'
echo 'abc' == 'ABC'
set ignorecase
echo 'abc' == 'ABC'
echo 'abc' =~ 'ABC'
set noignorecase
echo 'abc' =~# 'ABC'
echo [1,2] == [1,2]
echo {'a':1} == {'a':1}
echo 1 is 1
echo [1] is [1]
let x = [1]
echo x is x
echo 'a' < 'B'
echo 'a' <# 'B'
echo 2 == 2.0
echo '2' == 2
echo v:null == 0
echo v:true == 1
echo string(v:null) . string(v:none)
echo 0 || 1
echo 1 && 0
echo !0 . !1
echo -'3'
echo +'3'
echo 'a'[0]
echo 'abc'[1:]
echo 'abc'[-1]
echo [1,2,3][1:]
echo [1,2,3][-1]
echo [1,2,3][5:6]
echo {'a':1}['a']
echo 3 . 4
echo 3 .. 4
echo "x" =~ '\v^x$'
echo and(5,3) . or(5,3) . xor(5,3) . invert(0)
echo float2nr(3.9) . float2nr(-3.9)
echo string(round(2.5)) . string(trunc(-2.5)) . string(ceil(2.1)) . string(floor(2.9))
echo string(abs(-3)) . string(abs(-3.5))
echo string(pow(2,10)) . string(sqrt(16.0))
echo string(fmod(10.0,3.0))
