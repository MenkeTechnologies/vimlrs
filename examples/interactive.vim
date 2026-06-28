" interactive.vim — read input from the terminal, with embedded unit tests.
"
" Demonstrates the input family: input() (line read), inputlist() (numbered
" menu), confirm() (button choice). In the editor these prompt through the
" command-line UI; standalone they write the prompt to stdout and read one line
" from stdin — the role `read` plays in a shell script.
"
" The CI run feeds canned answers via tests/golden/interactive.in, so the
" embedded asserts check the parsed values; exits non-zero on failure.
"
" Interactive:   vimlrs examples/interactive.vim
" Scripted:      printf 'Ada\n2\ny\n' | vimlrs examples/interactive.vim

let name = input('What is your name? ')
echo 'Hello,' name . '!'

let choice = inputlist(['Pick a language:', '1. VimL', '2. Rust', '3. C'])
let langs = ['', 'VimL', 'Rust', 'C']
if choice >= 1 && choice <= 3
  echo 'Great choice:' langs[choice]
else
  echo 'No selection.'
endif

let ok = confirm('Save your answers?', "&Yes\n&No")
echo ok == 1 ? 'Saved.' : 'Discarded.'

" ── unit tests (valid only under the scripted stdin fixture: Ada / 2 / 1) ──
if !empty(name)
  call assert_equal('Ada', name)
  call assert_equal(2, choice)
  call assert_equal('Rust', langs[choice])
  call assert_equal(1, ok)

  if !empty(v:errors)
    for e in v:errors
      echo 'FAIL:' e
    endfor
    throw len(v:errors) . ' assertion(s) failed'
  endif
  echo 'OK: interactive assertions passed'
endif
