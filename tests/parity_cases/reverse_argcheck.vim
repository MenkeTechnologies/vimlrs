" reverse() rejects anything that is not a String, List, Tuple or Blob. The port
" had dropped the argument check that is the FIRST statement of the C
" (`tv_check_for_string_or_list_or_blob_arg`, vendor/eval/list.c:828), so
" `reverse(10)` silently returned 0 — accepting a call vim refuses.
"
" Found by the expression fuzzer, in the bucket it labels `Divergent`: vim and
" neovim disagree on the message (E1253 vs E1252), which is what put it there,
" but vimlrs matched NEITHER. A vim-vs-neovim split does not imply vimlrs is
" right; each one still has to be read.
for e in ['reverse(10)', 'reverse(1.0)', 'reverse(v:none)', 'reverse(v:null)', 'reverse(v:true)', "reverse({'a':1})", "reverse(function('strlen'))"]
  let r = 'no error'
  try
    let r = string(eval(e))
  catch
    let r = substitute(v:exception, '^Vim([a-z]*):', '', '')
  endtry
  echo e ' => ' r
endfor

" The accepted types are unchanged, including reversal in place for the two
" mutable ones.
echo 'string ' reverse('abc')
echo 'list   ' string(reverse([1, 2, 3]))
echo 'blob   ' string(reverse(0z010203))
let l = [1, 2, 3]
echo 'inplace' string(reverse(l) is l)
echo 'empty  ' string(reverse([])) string(reverse(''))
