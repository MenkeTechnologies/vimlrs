" c: eval5 checks the left operand of `+`, `-`, `.` and `<<`/`>>` BEFORE it
" parses the right one, "to avoid side effects after an error": a refused left
" operand means the right operand never runs.
let l = [1, 2, 3, 4]
echo [] - remove(l, 0)
echo l
echo {} . remove(l, 0)
echo l
echo 'a' << remove(l, 0)
echo l
echo 1.5 >> remove(l, 0)
echo l
" `+` lets a List/Blob through (the right one may be a List too), so the right
" operand does run there.
echo 0z00 + remove(l, 0)
echo l
