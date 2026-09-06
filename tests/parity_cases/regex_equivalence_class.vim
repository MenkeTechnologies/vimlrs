" c: `get_equi_class` / `get_coll_element` (`regexp.c:464`) accept EXACTLY ONE
" character between the delimiters, so `[[=ab=]]` is no equivalence class at all
" and its `[` falls through to a literal. A real one then expands through
" `nfa_emit_equi_class`'s per-letter table (`regexp_nfa.c:696`), which this
" engine did not have: `[[=a=]]` matched only the bare `a`.
for c in [['[[=a=]]','a'], ['[[=a=]]','á'], ['[[=a=]]','à'], ['[[=a=]]','â'], ['[[=a=]]','ä'], ['[[=a=]]','å'], ['[[=a=]]','Á'], ['[[=a=]]','x']]
  echo c[0] . ' ' . c[1] . ' ' . match(c[1], c[0])
endfor
for c in [['[[=A=]]','Ä'], ['[[=A=]]','ä'], ['[[=e=]]','é'], ['[[=n=]]','ñ'], ['[[=y=]]','ÿ'], ['[[=z=]]','ž'], ['[[=o=]]','ø'], ['[[=c=]]','ç']]
  echo c[0] . ' ' . c[1] . ' ' . match(c[1], c[0])
endfor
" A multi-character body is not a class, and neither is a collating element.
for c in [['[[=ab=]]','a'], ['[[=ab=]]','b'], ['[[.ab.]]','a'], ['[[.a.]]','a'], ['[[=a=]]','']]
  echo c[0] . ' ' . c[1] . ' ' . match(c[1], c[0])
endfor
echo string(matchstr('a]b', '[[=a=]]'))
echo string(matchstr('aa', '[[=a=]'))
