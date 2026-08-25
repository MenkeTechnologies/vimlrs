" v:errmsg — set by emsg() at vendor/message.c:813, which sits AFTER the
" cause_errthrow return and BEFORE the emsg_silent return. So: a caught error
" does NOT record it, a :silent! one DOES, and a plain one DOES.
echo 'initial=[' . v:errmsg . ']'

try
  echo [1] . 'x'
catch
  echo 'caught=[' . v:exception . ']'
endtry
echo 'after-catch=[' . v:errmsg . ']'

silent! echo [][0]
echo 'after-silent=[' . v:errmsg . ']'

silent! call NoSuchFunctionHere()
echo 'after-silent-call=[' . v:errmsg . ']'

let v:errmsg = ''
echo 'cleared=[' . v:errmsg . ']'

echo strlen([1])
echo 'after-plain=[' . v:errmsg . ']'

echo 'ok'
echo 'unchanged-by-ok=[' . v:errmsg . ']'

let v:errmsg = 'assigned by hand'
echo 'assigned=[' . v:errmsg . ']'

" A second error overwrites it rather than appending.
silent! echo 1 + {}
echo 'overwritten=[' . v:errmsg . ']'
