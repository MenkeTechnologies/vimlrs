" An error inside execute() is shown AND captured, the call still returns what
" it captured, and emsg() switches messages back on for the rest of the run.
echo 1
let r = execute('let zz += 1')
echo 'r=' string(r)
echon 'x'
let r = execute(['echo 7', 'let yy += 1', 'echo 8'])
echo 'r=' string(r)
let r = execute('let qq += 1', '')
echo 'r=' string(r)
let r = execute('let qq += 1', 'silent!')
echo 'r=' string(r)
" An outer :silent! hides the error but still writes it into the capture.
silent! let r = execute('let zz += 1')
echo 'r=' string(r)
echo execute('silent! let zz += 1', 'silent')
" {silent} "" shows the output as well; the next :echo still breaks the line.
let r = execute('echo 7', '')
echon 'n'
echo 9
function F()
  return execute('let zz += 1')
endfunction
echo string(F())
try
  let r2 = execute('let zz += 1')
catch
  echo 'caught' v:exception
endtry
echo exists('r2')
