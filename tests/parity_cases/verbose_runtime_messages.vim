" The harness sources at 'verbose' 1 (`verbose source`), so :runtime reports
" what it did not find (c: do_in_path), and a 'spelllang' that is set sources
" spell/{lang}.vim (c: do_spelllang_source).
echo &verbose &vbs
set verbose?
runtime nosuch.vim
runtime! a.vim b.vim
runtime START nosuch.vim
runtime ALL nosuch.vim
runtime! ALL nosuch.vim
runtime OPT x.vim
runtime PACK x.vim
runtime S x.vim
runtime! syntax/nosuch*.vim
runtime A
set spl=de_de
set spl=cjk,fr.utf-8
set spl=
let &spl = 'en'
set spl=-x
set spl=_x
set vbs=0
runtime nosuch.vim
set vbs=1
runtime  x.vim  
