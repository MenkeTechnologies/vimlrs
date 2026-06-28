" heredoc.vim — `:let var =<< [trim] END` here-document assignment.
"
" `:let x =<< MARKER` reads the lines that follow, verbatim, up to a line equal
" to MARKER, and assigns them as a List of strings. `trim` removes the leading
" indentation of the first body line from every line (and lets the end marker be
" indented to match the `:let`). Mirrors heredoc_get() in Vim. Self-tests into
" v:errors; exits non-zero on failure.
"
"   vimlrs examples/heredoc.vim

" ── plain heredoc → List of lines ──
let basic =<< END
hello
world
END
call assert_equal(['hello', 'world'], basic)

" ── body is verbatim: quotes, bars and other punctuation are literal ──
let raw =<< EOF
it's got a | pipe
a "quoted" word
END is only the marker when alone
EOF
call assert_equal(["it's got a | pipe", 'a "quoted" word',
      \ 'END is only the marker when alone'], raw)

" ── empty heredoc → empty List ──
let none =<< END
END
call assert_equal([], none)

" ── trim: the first body line's indent is stripped from every line, and the
"    relative indent of deeper lines is preserved ──
function! Trimmed() abort
  let l =<< trim END
    root
      child
    root2
  END
  return l
endfunction
call assert_equal(['root', '  child', 'root2'], Trimmed())

" ── a heredoc can target a Dict entry, like any :let lvalue ──
let doc = {}
let doc.body =<< END
first
second
END
call assert_equal(['first', 'second'], doc.body)

" ── it composes with normal code on either side ──
let before = 1
let payload =<< END
data
END
let after = 2
call assert_equal(1, before)
call assert_equal(2, after)
call assert_equal(['data'], payload)

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: heredoc assertions passed'
