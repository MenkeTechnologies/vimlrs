" exists(':cmd') and fullcommand() over the real Ex-command table.
"
" c: cmd_exists() (ex_docmd.c:3226 upstream; its call site is vendored at
" vendor/eval/funcs.c:1388) checks the command MODIFIERS first, on their own
" minimum abbreviation (c:3229-3239), then resolves through find_ex_command
" (c:3057). 2 means the name was given in full, 1 that an abbreviation was
" accepted, 0 neither.
"
" The abbreviation rule is TABLE ORDER, not a per-command minimum length:
" find_ex_command walks cmdnames[] forward and takes the FIRST entry whose name
" starts with what was typed (c:3130-3139). That is why :s is substitute and not
" sort, and :co is copy while :con is continue.
"
" Only commands BOTH engines have are probed here. This port carries Neovim's
" table, so the nvim-only names (:checkhealth, :detach, :packadd siblings, …)
" and vim-only ones answer differently by construction — see BUGS.md.

" ── the table-order rule ──
echo 's        ' exists(':s') fullcommand('s')
echo 'su       ' exists(':su') fullcommand('su')
echo 'sub      ' exists(':sub') fullcommand('sub')
echo 'substitute' exists(':substitute') fullcommand('substitute')
echo 'sor      ' exists(':sor') fullcommand('sor')
echo 'sort     ' exists(':sort') fullcommand('sort')
echo 'co       ' exists(':co') fullcommand('co')
echo 'con      ' exists(':con') fullcommand('con')
echo 'cont     ' exists(':cont') fullcommand('cont')
echo 'ret      ' exists(':ret') fullcommand('ret')
echo 'retu     ' exists(':retu') fullcommand('retu')
echo 'bre      ' exists(':bre') fullcommand('bre')
echo 'brea     ' exists(':brea') fullcommand('brea')
echo 'fin      ' exists(':fin') fullcommand('fin')
echo 'fina     ' exists(':fina') fullcommand('fina')
echo 'fo       ' exists(':fo') fullcommand('fo')
echo 'for      ' exists(':for') fullcommand('for')
echo 'tr       ' exists(':tr') fullcommand('tr')
echo 'try      ' exists(':try') fullcommand('try')
echo 'el       ' exists(':el') fullcommand('el')
echo 'i        ' exists(':i') fullcommand('i')
echo 'r        ' exists(':r') fullcommand('r')
echo 're       ' exists(':re') fullcommand('re')
echo '--1'

" ── one_letter_cmd: :k and the :s family are ONE character, whatever follows ──
echo 'k        ' exists(':k') fullcommand('k')
echo 'si       ' exists(':si') fullcommand('si')
echo 'sig      ' exists(':sig') fullcommand('sig')
echo 'sign     ' exists(':sign') fullcommand('sign')
echo 'sim      ' exists(':sim') fullcommand('sim')
echo 'sil      ' exists(':sil') fullcommand('sil')
echo 'sI       ' exists(':sI') fullcommand('sI')
echo 'sg       ' exists(':sg') fullcommand('sg')
echo 'sr       ' exists(':sr') fullcommand('sr')
echo 'sre      ' exists(':sre') fullcommand('sre')
echo 'ke       ' exists(':ke') fullcommand('ke')
echo 'kee      ' exists(':kee') fullcommand('kee')
echo '--2'

" ── :dl / :dp are :d with the l/p flag, so the flag is not part of the name ──
echo 'd        ' exists(':d') fullcommand('d')
echo 'dl       ' exists(':dl') fullcommand('dl')
echo 'dp       ' exists(':dp') fullcommand('dp')
echo 'del      ' exists(':del') fullcommand('del')
echo 'delete   ' exists(':delete') fullcommand('delete')
echo 'dli      ' exists(':dli') fullcommand('dli')
echo '--3'

" ── :ho is forced unresolved: as a MODIFIER `horizontal` needs three chars ──
echo 'ho       ' exists(':ho') fullcommand('ho')
echo 'hor      ' exists(':hor') fullcommand('hor')
echo 'horizontal' exists(':horizontal') fullcommand('horizontal')
echo '--4'

" ── the command modifiers, on their own minimum abbreviation ──
echo 'sil-mod  ' exists(':sil') exists(':silent') exists(':si')
echo 'verb     ' exists(':ver') exists(':verb') exists(':verbose')
echo 'vert     ' exists(':ve') exists(':vert') exists(':vertical')
echo 'bot      ' exists(':bo') exists(':bot') exists(':botright')
echo 'top      ' exists(':to') exists(':top') exists(':topleft')
echo 'abo      ' exists(':ab') exists(':abo') exists(':aboveleft')
echo 'keepj    ' exists(':keepj') exists(':keepjumps')
echo 'rightb   ' exists(':rightb') exists(':rightbelow')
echo '--5'

" ── the non-alpha commands ──
echo 'bang     ' exists(':!') fullcommand('!')
echo 'amp      ' exists(':&') fullcommand('&')
echo 'lt       ' exists(':<') fullcommand('<')
echo 'gt       ' exists(':>') fullcommand('>')
echo 'eq       ' exists(':=') fullcommand('=')
echo 'at       ' exists(':@') fullcommand('@')
echo 'tilde    ' exists(':~') fullcommand('~')
echo 'hash     ' exists(':#') fullcommand('#')
echo '--6'

" ── :2match / :3match: a leading digit is only allowed for :match ──
echo '2match   ' exists(':2match') fullcommand('2match')
echo '3match   ' exists(':3match') fullcommand('3match')
echo '2echo    ' exists(':2echo')
echo '1match   ' exists(':1match')
echo '--7'

" ── trailing garbage, empty, unknown ──
echo 'garbage  ' exists(':echo foo') exists(':nosuchcommandhere') exists(':')
echo 'Upper    ' exists(':Zz')
echo '--8'

" ── user-defined commands, before and after definition ──
echo 'before   ' exists(':Foo') fullcommand('Foo')
command! -nargs=0 Foo echo 1
echo 'after    ' exists(':Foo') fullcommand('Foo')
echo 'prefix   ' exists(':Fo') fullcommand('Fo')
delcommand Foo
echo 'deleted  ' exists(':Foo')
echo '--9'

" ── a leading colon and a range are skipped by fullcommand() ──
echo 'colon    ' fullcommand(':echo') fullcommand('::echo')
echo 'range    ' fullcommand('%s') fullcommand('1,2d')
echo '--10'
