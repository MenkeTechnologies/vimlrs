" The exit status is a per-PROCESS latch — c: `ex_exitval = 1` in emsg()
" (vendor/message.c:855) — so an error reported early in a script makes it exit
" 1 however many commands run after it.
"
" The function below holds a curly-brace name this port's parser cannot read
" yet (vim parses a legacy body lazily and never complains), which sends the
" file through the statement-at-a-time fallback (source_tolerant). There every
" statement ran as its own chunk and reset the latch, so only an error in the
" LAST statement reached the exit status: this script exited 0 where vim exits 1.

echo [1] + 1
function! s:Lazy()
  let x = open_{'pos'}
endfunction
echo 'after'
