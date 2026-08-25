" An error inside a conditional skips the rest of it, up to and including the
" OUTERMOST open :if/:while/:for.
"
" c: `do_cmdline` clears `did_emsg` between command lines only while the
" condition stack is empty (ex_docmd.c:448-454), `ea.skip` (c:2027-2031) skips
" every command while it is set, and the :endwhile/:endfor backedge is guarded by
" `!did_emsg` (c:667). The same flag is the first disjunct of CHECK_SKIP
" (vendor/ex_eval.c:80-85), read by ex_if (c:865) and ex_while (c:1007).

echo '-- 1. error in a :while body: one iteration, rest of the body skipped'
let g:i = 0
while g:i < 3
  let g:i += 1
  echo [1] . 'x'
  echo 'NEVER-1'
endwhile
echo 'i=' . g:i

echo '-- 2. and the same for :for'
let g:n = 0
for x in [1, 2, 3]
  let g:n += 1
  echo [1] . 'x'
  echo 'NEVER-2'
endfor
echo 'n=' . g:n

echo '-- 3. an error in the :while CONDITION: zero iterations'
let g:i = 0
while strlen([1]) < 2 && g:i < 3
  let g:i += 1
endwhile
echo 'i=' . g:i

echo '-- 4. an error in the :for list: zero iterations'
let g:n = 0
for x in [strlen([1]), 5]
  let g:n += 1
endfor
echo 'n=' . g:n

echo '-- 5. an error in an :if condition skips the :else too'
if strlen([1])
  echo 'NEVER-TRUE'
else
  echo 'NEVER-FALSE'
endif
echo 'past the :if'

echo '-- 6. an error in an :if body resumes after the :endif'
if 1
  echo [1] . 'x'
  echo 'NEVER-3'
endif
echo 'past the :endif'

echo '-- 7. nested: the unwind goes out to the OUTERMOST block, not one level'
let g:o = 0
let g:i = 0
while g:o < 2
  let g:o += 1
  let g:i = 0
  while g:i < 2
    let g:i += 1
    echo [1] . 'x'
  endwhile
  echo 'NEVER-4'
endwhile
echo 'o=' . g:o . ' i=' . g:i

echo '-- 8. an :if nested in a :while unwinds the whole :while'
let g:n = 0
while g:n < 3
  let g:n += 1
  if g:n == 2
    echo [1] . 'x'
  endif
  echo 'body end n=' . g:n
endwhile
echo 'n=' . g:n

echo '-- 9. did_emsg does NOT persist past the outermost block'
let g:i = 0
while g:i < 2
  let g:i += 1
endwhile
echo 'i=' . g:i
if 1
  echo 'the next block still runs'
endif
