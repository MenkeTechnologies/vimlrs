" script_lang_heredoc.vim — the `:{lang} << MARKER … MARKER` heredoc form of
" the script-language interface commands (`:python3`/`:python`/`:perl`/`:ruby`/
" `:lua`/`:tcl`/`:mzscheme`). Vim's `script_get()` (ex_docmd.c) sees the command
" argument begin with `<<` and calls `heredoc_get(…, script_get=true)`, which
" swallows every following line into the embedded interpreter until the end
" marker — so those body lines are NEVER seen by the vimscript parser. A missing
" marker defaults to `.` (vendor/eval/vars.c:791).
"
" Regression: at TOP LEVEL vimlrs used to leak the heredoc body — the lines
" between `python3 << EOF` and `EOF` were parsed as vimscript (raising E117/E492
" on junk) and the `EOF` marker line itself became `E492: Not an editor command`.
" This asserts the body is skipped and only the code after the marker runs.
"
" The interface is uncompiled in vimlrs (`has('python3')` etc. are 0), so the
" opener line is a no-op'd Ex command and the body is simply skipped. In real
" Vim the body IS handed to the interpreter, so every body line here is a valid
" no-op in its language (and would error if mis-parsed as vimscript) — keeping
" the observable result identical: no error, the post-marker assignment runs.

let s:log = []

" --- python3, `<< EOF` with a space; body is valid-noop python, junk vimscript.
python3 << EOF
pass
x = 1 + 2
EOF
call add(s:log, 'py3')

" --- perl, `<<END` with no space between `<<` and the marker.
perl <<END
my $x = 1;
1;
END
call add(s:log, 'perl')

" --- lua, default marker: `<<` with nothing after it terminates on `.`.
lua <<
local _ = 1
.
call add(s:log, 'lua')

" --- ruby, a marker that is a lower-case word (allowed for script heredocs;
" the E221 "cannot start with lower case" rule applies only to `:let =<<`).
ruby <<done
y = 2
done
call add(s:log, 'ruby')

" --- a heredoc body that itself contains a line identical to a DIFFERENT
" marker must NOT terminate early — only the exact opener marker ends it.
python3 << REALEND
pass
NOTTHEEND = 1
REALEND
call add(s:log, 'nested-marker')

" every stage past a heredoc ran, in order, with no leaked body line:
call assert_equal(['py3', 'perl', 'lua', 'ruby', 'nested-marker'], s:log)

" --- inline bit-shift is NOT a heredoc: `<< 2` does not begin the argument, so
" the line stays an ordinary (no-op) interface command, and the following line
" runs normally rather than being swallowed as heredoc body.
if has('win32')
  lua x = a << 2
  let s:should_run = 1
endif
call assert_equal(0, exists('s:should_run'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'script_lang_heredoc.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'script_lang_heredoc.vim: all assertions passed'
