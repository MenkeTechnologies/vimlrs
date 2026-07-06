" script_lang_cmds.vim — Vim's script-language interface Ex commands
" (`:python`/`:py3`/`:ruby`/`:perl`/`:lua`/`:tcl`/`:mzscheme` and their
" `do`/`file` variants). These run an embedded interpreter. Each MUST parse as an
" Ex command — never fall through to expression evaluation. The load-bearing case
" is ftplugin/ruby.vim, which puts a `ruby …` line inside a not-taken
" `if has('ruby') && has('win32')` branch: if that line fails to parse, the
" tolerant parser drops the WHOLE enclosing `:if` block, so the `else` branch
" that assigns `s:ruby_path` never runs and every later use is E121.
"
" This build (and vimlrs) has `has('win32')` == 0, so every interface command
" below sits in a branch that is never executed — the commands are exercised for
" PARSE recognition only, keeping the test identical in vim (which has the
" interpreters) and vimlrs (which has none). What is asserted is that the
" enclosing block structure survives, i.e. the sibling/else branch runs.

let s:reached = 0

" --- the ftplugin/ruby.vim shape: a `:ruby` line in a not-taken branch must not
" break the enclosing block, so the `else` branch runs and assigns the sentinel.
if has('ruby') && has('win32')
  ruby ::VIM::command( 'let s:ruby_path = split("%s",",")' % $:.join(%q{,}) )
else
  let s:ruby_path = 'from-else'
endif
call assert_equal('from-else', s:ruby_path)

" --- every interface command word (bare, digit-suffixed, and do/file variants)
" parses as an Ex command inside the not-taken branch; the block still closes and
" the code after `endif` runs, so the sentinel advances.
if has('win32')
  ruby puts 'hi'
  rubydo $_.upcase!
  rubyfile /tmp/x.rb
  python x = 1
  py x = 1
  pyfile /tmp/x.py
  python3 x = 1
  py3 x = 1
  py3file /tmp/x.py
  pythonx x = 1
  pyx x = 1
  perl print "x"
  perldo $_ = 1
  lua print(1)
  luado return 1
  luafile /tmp/x.lua
  tcl puts a
  tcldo set x 1
  mzscheme (display 1)
  mz (display 1)
endif
let s:reached += 1
call assert_equal(1, s:reached)

" --- a `:python3` call parses inside a function body without aborting the
" `:function` definition (it would otherwise fall through to expression parsing,
" leaking the body to global scope). The branch is not taken at runtime.
function! s:InBody() abort
  if has('win32')
    python3 vim.command('let unused = 1')
    ruby VIM::command('let unused = 1')
  endif
  return 'ok'
endfunction
call assert_equal('ok', s:InBody())

" --- a builtin whose name merely starts with an interface prefix is NOT the
" command: `luaeval(...)` / `py3eval(...)` stay funcref-call expressions, so the
" function references still resolve.
call assert_equal(1, exists('*luaeval'))
call assert_equal(1, exists('*py3eval'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'script_lang_cmds.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'script_lang_cmds.vim: all assertions passed'
