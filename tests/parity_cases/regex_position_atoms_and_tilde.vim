" `:help /~` — `~` is the last given substitute string, and with none it is E33.
" (`substitute()` does not set one; only the `:s` command does, and this
" interpreter has no `:s`.)
try
  call match('a', '~')
catch
  echo substitute(v:exception, '^Vim(\w*):', '', '')
endtry
" Under `\M`/`\V` a bare `~` is an ordinary character.
echo match('a~b', '\M~')
echo match('a~b', '\V~')

" `:help /\%^` `/\%$` — the start and end of the file, which for a one-string
" match are the ends of the subject.
echo match('ab', '\%^') . ' ' . match('ab', '\%$') . ' ' . match('ab', '\%^b')
echo matchstr('ab', '\%^a') . matchstr('ab', 'b\%$')
echo match('', '\%^') . ' ' . match('', '\%$')

" `:help /\%c` — a 1-based BYTE column, and its `>` / `<` forms.
echo match('a', '\%1c') . ' ' . match('ab', '\%2c') . ' ' . match('ab', '\%3c')
echo match('ab', '\%0c') . ' ' . match('ab', '\%>1c') . ' ' . match('ab', '\%<2c')
echo match('a', '.\{}\%1c')
" A line number and a virtual column need a buffer; nothing in a string match
" satisfies them.
echo match('ab', '\%1l') . ' ' . match('ab', '\%V')

" An unterminated `[` is literal text, and `[=a=]` inside one is a single item
" carrying its own `]` — that `]` does not close the collection.
echo matchstr('aa', '[[=a=]') . '|' . matchstr('a]b', '[[=a=]') . '|'
echo matchstr('a]b', '[[=a=]]')

" A trailing lone backslash escapes nothing and matches the character itself.
echo match('a\b', '\') . ' ' . matchstr('a\b', '\')
