" What abandons the rest of a `|`-separated command line, and what does not.
"
" c: the C has two independent reasons. `ea.skip` (ex_docmd.c:2027-2031) skips
" every command after one that REPORTED an error, which `:silent!` prevents by
" keeping did_emsg clear (vendor/message.c:817-846). And separately, a command
" whose ARGUMENT failed to parse or evaluate never sets eap->nextcmd, so the
" rest of the line is dropped whether or not anything was reported — while a
" command that consumed its argument and then failed to ACT (eexe_mod_op's E734,
" a callee's body erroring) has already set it and the line carries on.
"
" That second half is the whole point of the silenced rows below: this port used
" to abandon the line for every silenced hard failure, which is right for two of
" them and wrong for the other three.

function! C()
  return len([1] . '')
endfunction
silent! let g:a = C() | echo 'A ran'
echo '--1'
silent! let g:b = [] . '' | echo 'B ran'
echo '--2'
let g:c = 1 | echo 'C ran'
echo '--3'
silent! echo strlen([1]) | echo 'D ran'
echo '--4'
echo strlen([1]) | echo 'E ran'
echo '--5'
silent! call nosuchfunc_zz() | echo 'F ran'
echo '--6'
silent! let g:g = {x -> len([1] . '')}(1) | echo 'G ran'
echo '--7'

" The command-level E734 family: the argument was consumed, so the line runs on.
let g:d = {'a': 1}
silent! let g:d += [1] | echo 'H ran'
echo '--8'
let @r = 'x'
silent! let @r += 1 | echo 'I ran'
echo '--9'
set textwidth=10
silent! let &textwidth .= 'x' | echo 'J ran'
echo '--10'
let $VIMLRS_LA = 'e'
silent! let $VIMLRS_LA += 1 | echo 'K ran'
echo '--11'

" A locked variable, an undefined variable and a bad subscript all report AFTER
" the argument was consumed, so the line carries on.
let g:lk = [1]
lockvar g:lk
silent! let g:lk += [2] | echo 'L ran'
echo '--12'
unlockvar g:lk
silent! let g:m = g:nosuchvar_la | echo 'M ran'
echo '--13'
silent! let g:n = [1][9] | echo 'N ran'
echo '--14'

" `:call` is the exception: it sets nextcmd only when the call succeeded, so a
" failed one drops the line even though the argument was consumed — while the
" same failing call under `:echo` does not.
silent! call nosuchfn_la() | echo 'O ran'
echo '--15'
silent! echo nosuchfn_la() | echo 'P ran'
echo '--16'
silent! let g:q = nosuchfn_la() | echo 'Q ran'
echo '--17'
