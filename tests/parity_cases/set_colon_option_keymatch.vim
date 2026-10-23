" +=, ^= and -= on a P_COMMA|P_COLON option ('wildmode') with a key:value item
" or several items work item by item (c: stropt_handle_keymatch, vim 9.2).
for c in ['wim=full', 'wim^=list,', 'wim+=longest:full', 'wim+=longest:list',
      \ 'wim^=list:full,full', 'wim-=longest:', 'wim-=list:x,full',
      \ 'wim=list:full,full,list:longest,noselect', 'wim+=list:full',
      \ 'wim-=noselect,', 'wim+=,', 'wim=', 'wim+=full,list', 'wim^=a:b',
      \ 'wim=full,list:full,list:x', 'wim+=list:full', 'wim+=noinsert',
      \ 'wim+=noinsert', 'wim-=noinsert']
  try
    exe 'set ' . c
  catch
    echo v:exception
  endtry
  echo c '->' &wim
endfor
