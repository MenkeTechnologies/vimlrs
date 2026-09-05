" An unmatched group is reported in the dialect the pattern was WRITTEN in, and
" `\%(` carries its own E-number.
"   c: EMSG_M_RET_NULL(e_unmatched_str_open, …) formats the message with "" when
"   reg_magic == MAGIC_ALL and "\\" otherwise.
for s:p in ['\(', '\v(', '\M\(', '\V\(', '\)', '\v)', '\M\)', '\%(', '\v%(', '\v(a', '\v(a))']
  try
    call match('a', s:p)
  catch
    echo s:p . ' -> ' . substitute(v:exception, '^Vim(\w*):', '', '')
  endtry
endfor
