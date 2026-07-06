" foldcmds.vim — the fold-view Ex commands (ex_docmd.c: ex_fold / ex_foldopen).
" `:fo[ld]` creates a fold over a range; `:foldo[pen][!]` opens folds and
" `:foldc[lose][!]` closes them (the `!` is recursive). All of these act on a
" window's fold view. A standalone eval engine (no window, no fold state) has
" nothing to open or close, so every form is a silent no-op — but each MUST
" parse as an Ex command, not fall through to expression evaluation (which would
" raise E121: Undefined variable / E492: Not an editor command on the fold word).
" Real Vim sources syntax/cdl.vim's whole-file `%foldo!` silently once its
" `set foldmethod=expr` has created folds; vimlrs reproduces that silence.
" Self-test: a sentinel is set only if every documented form is reached.

let s:reached = 0

" --- :fold create abbreviations (fo / fol / fold), bare and with a range
fo
fol
fold
1,1fold
let s:reached += 1

" --- :foldopen abbreviations (foldo .. foldopen), bang and range forms
foldo
foldo!
foldop
foldope
foldopen
foldopen!
%foldo!
let s:reached += 1

" --- :foldclose abbreviations (foldc .. foldclose), bang and range forms
foldc
foldc!
foldcl
foldclo
foldclos
foldclose
foldclose!
%foldclose!
let s:reached += 1

" --- parses inside a function body (must not abort the :function definition).
" `:fold` creates a fold and is silent even with no existing folds, so this
" returns cleanly in both vim and vimlrs. (`:foldopen`/`:foldclose` would set
" E490 "No fold found" in real vim when no folds exist, aborting the `abort`
" function — that divergence is fold-state, not command recognition, so it is
" out of scope here.)
function! s:Folds() abort
  set foldmethod=manual
  fold
  return 'folded'
endfunction
call assert_equal('folded', s:Folds())

" --- a bare fold word at statement start is the command, never a variable read
call assert_equal(3, s:reached)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'foldcmds.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'foldcmds.vim: all assertions passed'
