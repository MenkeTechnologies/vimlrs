" syntax_pattern_bar.vim — a `|` inside a `:sy[ntax]` `/…/` pattern is literal
" regex alternation, NOT a command separator. Vim's syntax parser skips each
" pattern with skip_regexp before it looks for a trailing `|`, so bars inside
" the delimiters never split the command. A `|` AFTER the closing delimiter
" still starts the next command (do_one_cmd). Regression: vimlrs split on the
" inner bar and ran `G`/`E`/… as bare Ex commands → `E492: Not an editor
" command: G`. The absence of any E492 on stderr is the primary pin (the test
" harness fails on any `E<digits>:` line); the asserts pin the split behaviour.
" Self-test: asserts into v:errors, throws if any failed.

" --- inner bars are literal: the exact dot.vim:39 pattern (no trailing cmd).
"     A bar-split here would emit `E492: Not an editor command: G`.
syn match dotEscString /\v\\(N|G|E|T|H|L)/ containedin=dotString

" --- inner bar with a `\|` alternation and a char class containing `|`
"     (jq.vim jqOperator style). Still one command, no split.
syn match barClass /:\|[!|]=/

" --- a `|` AFTER the closing `/` DOES separate: the trailing command runs.
let g:after_bar = 0
syn match barAfter /a\|b/ | let g:after_bar = 1
call assert_equal(1, g:after_bar)

" --- scoping: a real `/` division outside :syntax still splits on the bar,
"     so both `let`s run (the fix must not treat every `/` as a delimiter).
let g:d1 = 0 | let g:d2 = 0
let g:d1 = 8 / 2 | let g:d2 = 9 / 3
call assert_equal(4, g:d1)
call assert_equal(3, g:d2)

" --- region patterns: bars inside start=/…/ end=/…/ are literal too.
syn region barRegion start=/(\|\[/ end=/)\|\]/

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'syntax_pattern_bar.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'syntax_pattern_bar.vim: all assertions passed'
