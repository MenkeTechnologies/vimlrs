" width.vim — display width and character classes, with embedded unit tests.
"
" Demonstrates strwidth() (display cells: wide CJK/emoji count as 2, composing
" marks as 0), strdisplaywidth() (Tabs expand to the next 'tabstop'), and
" charclass() (0 blank, 1 punctuation, 2 word, 3 emoji). Self-checks and exits
" non-zero on failure.
"
"   vimlrs examples/width.vim

" ── strwidth(): one cell per ASCII char, two per wide char ──
call assert_equal(5, strwidth('hello'))
call assert_equal(0, strwidth(''))
call assert_equal(6, strwidth('日本語'))
" A base char + a composing mark occupies one cell (the mark adds 0).
call assert_equal(1, strwidth('e' . nr2char(769)))

" ── strdisplaywidth(): a Tab advances to the next 'tabstop' boundary ──
set tabstop=8
call assert_equal(8, strdisplaywidth("\t"))
call assert_equal(9, strdisplaywidth("a\tb"))
call assert_equal(4, strdisplaywidth('abcd'))

" ── charclass(): the class of the first character ──
call assert_equal(0, charclass(' '))
call assert_equal(1, charclass('!'))
call assert_equal(2, charclass('a'))
call assert_equal(2, charclass('_'))
call assert_equal(2, charclass('5'))
call assert_equal(0, charclass(''))

" ── demo: align a column of CJK + ASCII labels by display width ──
for label in ['id', '名前', 'x']
  let pad = repeat(' ', 6 - strwidth(label))
  echo label . pad . '| value'
endfor

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: width assertions passed'
