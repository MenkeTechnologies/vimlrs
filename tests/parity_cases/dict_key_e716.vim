" E716 quotes the key. Every site goes through one format string —
" `semsg(_(e_dictkey), key)` at vendor/eval.c:901/3346, vendor/eval/funcs.c:3250,
" vendor/eval/userfunc.c:2694/3568 and vendor/eval/typval.c:3355 — so the four
" sites that printed the key bare were all wrong the same way.
let d = {'a': 1}
try
  echo d['b']
catch
  echo v:exception
endtry
try
  echo d.b
catch
  echo v:exception
endtry
try
  call d.nokey()
catch
  echo v:exception
endtry
try
  unlet d.nokey
catch
  echo v:exception
endtry
try
  echo d.b.c
catch
  echo v:exception
endtry
" A key that is not a valid identifier, and one holding a quote, to pin that the
" quoting is literal and not an escape pass.
try
  echo d['a b']
catch
  echo v:exception
endtry
try
  echo d['q"q']
catch
  echo v:exception
endtry
