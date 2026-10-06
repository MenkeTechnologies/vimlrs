" Two method-call forms of eval7.
"
" `expr->{lambda}(args)` — c: eval_lambda() calls the lambda with `expr`
" prepended to the arguments. It used to drop the whole line.
"
" A number literal takes its `-`/`+` leaders BEFORE the subscripts that follow
" it — c: the number case of eval7, "Apply prefixed "-" and "+" now. Matters
" especially when "->" follows", via eval7_leader(…, numeric_only = true, …),
" which stops at the first `!`. So `-1->abs()` is abs(-1) = 1, where it was
" -(abs(1)) = -1. Any other operand keeps its leaders until after the chain.

let s = 'x'
echo s->{v -> v . 'y'}()
echo [1, 2]->{l, n -> l + [n]}(3)
echo 5->{a, b -> a - b}(2)->{v -> v * 10}()
echo 'abc'->{s -> toupper(s)}()->strlen()
echo [3]->{-> 'noargs'}()

echo -1->abs()
echo -1.5->abs()
echo --3->abs()
echo +-4->abs()
echo !-1->abs()
echo -!1->string()
let n = 1
echo -n->abs()
echo -[1]->len()
echo -'3'->str2nr()
