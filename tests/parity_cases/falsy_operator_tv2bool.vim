" `??` tests its left operand with tv2bool() (vendor/eval.c:1905), not the
" tv_get_number() of `?:` — a non-empty String/List/Dict/Blob is truthy and
" is the result; it used to read 'a' as the Number 0 and take the right side.
echo 0 ?? 'a' '' ?? 'b' 'x' ?? 'c' [] ?? 'd' [0] ?? 'e' {} ?? 'f' {'a':1} ?? 'g'
echo 0.0 ?? 'h' 0.5 ?? 'i' v:false ?? 'j' v:null ?? 'k' 0z ?? 'l' 0z00 ?? 'm' '0' ?? 'n'
echo function('len') ?? 'o'
let x = 0 ?? '' ?? 'last'
echo x
