" interactive.vim — read input from the terminal in a standalone script.
"
" Demonstrates the input family: input() (line read with a prompt), inputlist()
" (numbered menu), and confirm() (button choice). In the editor these prompt
" through the command-line UI; standalone they write the prompt to stdout and
" read one line from stdin — the role `read` plays in a shell script.
"
" Pipe answers in for a non-interactive run:
"   printf 'Ada\n2\ny\n' | vimlrs examples/interactive.vim
" or just run it and type:
"   vimlrs examples/interactive.vim

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
if ok == 1
  echo 'Saved.'
else
  echo 'Discarded.'
endif
