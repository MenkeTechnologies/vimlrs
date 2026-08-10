" Option defaults that both reference engines report identically, and the
" agreement between `exists('&opt')` and reading `&opt`.
"
" Until round 30 every option outside a 21-row table read "" and answered
" `exists('&opt') == 0`, because two SEPARATE tables backed the two questions —
" `&opt` and `:set` resolve through `option::findoption`, `exists('&opt')` and
" `:let &opt` through `option_optval::find_option`. 'runtimepath' was in the
" first only, so `exists('&rtp')` said 0 while `&rtp` resolved; the other 90
" options here were in neither.
"
" Everything below was measured to be the SAME in `vim -N -es -u NONE -i NONE`
" and `nvim --clean --headless`, so no engine-specific or startup-specific state
" is pinned. `scripts/parity.sh` runs the oracle with `-N`, which is the state
" these defaults belong to: without it vim is in compatible mode and
" 'compatible', 'backspace', 'whichwrap', 'more', 'ruler' and 'showcmd' below
" all read differently.
"
" Deliberately ABSENT, because no constant is correct for them:
"   * the 27 options where vim and nvim genuinely disagree — 'cpoptions',
"     'formatoptions', 'history', 'shortmess', 'path', 'complete', 'listchars',
"     'laststatus', 'joinspaces', 'startofline', 'background', 'ttimeoutlen' …
"   * 'helplang' and 'fileencodings', which are derived from the LOCALE.
"     Measured on vim 9.2.0900: &helplang is '' under LC_ALL=C, 'en' under
"     en_US.UTF-8, 'de' under de_DE.UTF-8 and 'ja' under ja_JP.UTF-8;
"     &fileencodings is 'ucs-bom' under LC_ALL=C and
"     'ucs-bom,utf-8,default,latin1' otherwise. Neither harness pins a locale,
"     so a record of either would be a record of the developer's machine.
"   * 'runtimepath', whose value is a filesystem layout. Its `exists()` is
"     checked below; its value is not.

" ── booleans ────────────────────────────────────────────────────────────────
echo &compatible &backup &autowrite &binary &bomb &cindent &confirm
echo &copyindent &cursorline &digraph &endofline &equalalways &errorbells
echo &gdefault &infercase &insertmode &lisp &list &modeline &modifiable
echo &more &paste &preserveindent &readonly &ruler &shiftround &showcmd
echo &showmatch &smartindent &splitbelow &splitright &swapfile &tildeop
echo &title &visualbell &warn &wrapscan &writebackup
echo &emoji &exrc &icon &linebreak &termguicolors &ttyfast &undofile

" ── numbers ─────────────────────────────────────────────────────────────────
echo &maxfuncdepth &maxmapdepth &redrawtime &regexpengine &report
echo &timeoutlen &undolevels &updatetime &wildchar
echo &cmdheight &cmdwinheight &conceallevel &foldlevel &helpheight
echo &iminsert &imsearch &matchtime &numberwidth &pumheight &pumwidth
echo &scrolljump &showtabline &sidescrolloff &synmaxcol &updatecount
echo &winheight &winminheight &winwidth &wrapmargin &writedelay

" ── strings ─────────────────────────────────────────────────────────────────
echo &ambiwidth
echo &backspace
echo &breakat
echo &fileformats
echo &isident
echo &matchpairs
echo &selection
echo &suffixes
echo &whichwrap
echo &wildmode
echo &spelllang
echo '[' . &clipboard . &keymodel . &langmap . &spellfile . &virtualedit . ']'

" ── the abbreviation resolves to the same option ────────────────────────────
echo &cp &bs &ffs &mps &sel &ww &wim &ambw &brk &isi &spl
echo &mfd &mmd &rdt &re &tm &ul &ut &wc &ch &cwh &cole &fdl &hh
echo &imi &ims &mat &nuw &ph &pw &sj &stal &siso &smc &uc &wh &wmh &wiw &wm &wd
echo &bk &aw &bin &cin &cf &ci &cul &dg &eol &ea &eb &gd &inf &im &ml &ma
echo &pi &ro &ru &sr &sc &sm &si &sb &spr &swf &top &vb &ws &wb &emo &ex
echo &lbr &tgc &tf &udf

" ── `exists('&opt')` answers 1 for every option that has a value ────────────
"
" These two questions used to be answered by different tables. A `1` here with
" an empty read above, or the reverse, is the drift that gap was made of.
echo exists('&compatible') exists('&cp') exists('&backspace') exists('&bs')
echo exists('&undolevels') exists('&ul') exists('&selection') exists('&sel')
echo exists('&runtimepath') exists('&rtp') exists('&filetype') exists('&syntax')
echo exists('&nosuchoptionxyz') exists('&helplangy')

" ── and `:set` still round-trips through the same table ─────────────────────
set backspace=eol
echo &backspace &bs
set nocompatible
echo &compatible
set compatible
echo &compatible &cp
set undolevels=17
echo &undolevels &ul
set selection=exclusive
echo &selection &sel
