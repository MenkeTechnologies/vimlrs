" Every parse diagnostic this port raises, in vim's wording rather than in
" Rust's (BUGS.md R30-O5: "E15: expected RParen, found Eof" and friends).
" `eval()` is the vehicle because it is the one path that reaches the parser at
" RUN time, so the message is observable; a syntax error in the script text
" itself is still swallowed (R30-O1).
"
" `call eval(...)` inside try/catch, so what is recorded is the FIRST message —
" the specific diagnostic. Uncaught, `E15: Invalid expression: "<whole
" expression>"` follows it; that pair is pinned by `examples/parse_errors.vim`,
" not here, because this port does not yet echo the Number 0 a failed eval()
" answers (the statement-model gap, BUGS.md R26-O2 / R28-O1).
"
" Four probes are deliberately absent because vim and the Neovim porting spec
" word them differently and this port follows Neovim — they are in
" `examples/parse_errors.vim` instead, with the split spelled out:
"   * an unterminated quote — vim "Missing single/double quote", Neovim
"     "Missing quote";
"   * `f(` / `f(1` / `f(1,` — vim appends the unread source to the function
"     name in E116, Neovim does not.
for s:e in ['((1)', '(1', '[1', '[1,2', '[1 2]', "{'a' 1}", "{'a': 1",
      \ '#{a 1}', '#{a: 1', '{ x -> x', ']', '}', ')',
      \ '1 +', '1 .', '1 ?', '1 ? 2']
  try
    call eval(s:e)
    echo 'no error: ' . s:e
  catch
    echo v:exception
  endtry
endfor
