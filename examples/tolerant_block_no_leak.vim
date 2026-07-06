" tolerant_block_no_leak.vim — a block whose body vimlrs cannot yet parse is
" consumed WHOLE by the error-tolerant sourcer, never leaking its inner
" statements out to run at the top level (and never crashing).
"
" Regression: `:function`/`:while`/`:if`/… delimit a block that Vim reads to its
" terminator regardless of body contents (legacy functions parse lazily at call
" time). When a body construct is unsupported, the strict parse fails and vimlrs
" falls back to statement-by-statement sourcing. That fallback used to resume at
" the line AFTER the opener, so the function's inner `while` loop leaked out and
" executed at script scope — a top-level loop that fusevm block-JIT-compiled and
" jumped through, a hard SIGSEGV. The fix skips the whole block on a failed
" opener. Self-test into v:errors.

" --- A legacy function with an unsupported curly-brace name (`open_{pos}`) in a
"     `while` loop. If the body leaked, `g:leaked_from_function` would be set at
"     the top level; it must stay undefined because the block is skipped whole.
function! s:BrokenBody()
  let g:leaked_from_function = 1
  let pos = 0
  while pos != -1
    let x = open_{pos}
    let pos = -1
  endwhile
endfunction

call assert_false(exists('g:leaked_from_function'))

" --- A top-level statement after the skipped block still takes effect: the
"     sourcer resumes cleanly past the block terminator, not inside the body.
let g:after_block = 42
call assert_equal(42, g:after_block)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'tolerant_block_no_leak.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'tolerant_block_no_leak.vim: all assertions passed'
