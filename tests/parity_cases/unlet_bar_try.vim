" An error raised while resolving an `:unlet` ELEMENT lval takes the rest of the
" command line with it, so a `:catch` on that same line never runs and the
" exception escapes the one-line `:try` and aborts the script.
"
" `ex_unletlock()` (vendor/eval/vars.c) is the reason. The element form goes
" through `get_lval()`; when that fails the loop `break`s, which reaches
"
"   eap->nextcmd = check_nextcmd(arg);
"
" with `arg` still sitting on the UNCONSUMED name — so `check_nextcmd` finds no
" `|` there and answers NULL. No next command means the `| catch | … | endtry`
" that follows on the line is never executed.
"
" The plain-NAME form is the opposite, and the contrast is the point: its E108
" comes from the `callback` (`do_unlet`), which only sets `error = true`. The
" loop keeps going, `arg` advances to the `|`, `check_nextcmd` answers non-NULL,
" and the same-line `:catch` DOES run. `:let` with the same out-of-range index is
" caught too, for the same reason (`ex_let` always reaches its `check_nextcmd`
" with `arg` past the expression).
"
" Both engines agree on every line here.

let b = [1,2,3]

" Caught: the failure is in the unlet callback, not in the lval.
try | unlet nosuchvar | catch | echo 'C-name:' v:exception | endtry

" Caught: :let reaches its check_nextcmd with the cursor on the bar.
try | let b[9] = 1 | catch | echo 'C-let:' v:exception | endtry

" Caught: the multi-line form has no rest-of-line to abandon.
try
  unlet b[9]
catch
  echo 'C-multiline:' v:exception
endtry

" A negative index that is out of range does not error at all — it clamps to 0
" and removes the first item (see list_index_e684.vim).
let c = [1,2,3]
try | unlet c[-9] | catch | echo 'C-neg:' v:exception | endtry
echo 'after-neg:' string(c)

" NOT caught: the lval failed, the line was abandoned, the script aborts here.
" Nothing below this line runs, in any of the three.
try | unlet b[9] | catch | echo 'never-caught' | endtry
echo 'never-reached'
