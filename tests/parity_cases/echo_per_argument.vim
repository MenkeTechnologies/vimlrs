" :echo evaluates and WRITES one argument at a time (vendor/eval.c:6139-6186),
" so an error raised while evaluating argument N appears after argument N-1 has
" already been written — and, because emsg() reaches the display through
" msg_start(), argument N continues on the error's own line.

echo 'A =' strlen([1]) 'B'
echo '--1'

" eval1() FAILING for an argument breaks the loop (c:6146-6155): 'D' is dropped.
echo 'C' [1] . 'x' 'D'
echo '--2'

" A builtin that reports an error and still yields a value does NOT fail eval1,
" so its argument is printed and the ones after it too.
echo 'E' str2nr('0x1f', 0) 'F'
echo '--3'

" :echon has no separator and no leading line break.
echon 'G' strlen([1]) 'H'
echo ''
echo '--4'

" The failure of the FIRST argument prints nothing at all.
echo [1] . 'x' 'never'
echo '--5'

" Two failing arguments: only the first error is reported, the rest is dropped.
echo 'I' [1] . 'x' [2] . 'y'
echo '--6'

" A :silent! error is not reported but still fails the argument.
silent! echo 'J' [1] . 'x' 'K'
echo '--7'
echo 'errmsg=' v:errmsg
echo '--8'

" No error: plain multi-argument spacing, and the value forms echo renders.
echo 'L' 1 2.5 [1,2] {'a':1} v:null
echo '--9'

" execute() captures the joined arguments of each :echo as one message.
echo string(execute('echo "M" 1 2'))
echo string(execute('echon "N" 1 2'))
