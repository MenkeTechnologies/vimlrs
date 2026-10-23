" v:val / v:key exist only while a map()/filter() callback runs (c:
" prepare_vimvar/restore_vimvar around filter_map). Outside one they are not
" in v: at all, and an inner map() hands the outer callback its values back.
for n in ['key', 'val']
  try
    exe 'echo v:' . n
  catch
    echo v:exception
  endtry
  echo exists('v:' . n) has_key(v:, n) get(v:, n, 'absent')
  try
    exe 'let v:' . n . ' = 1'
  catch
    echo v:exception
  endtry
endfor
echo len(filter(keys(v:), 'v:val ==# "key" || v:val ==# "val"'))
echo map([7], 'has_key(v:, "val")')
echo map([7], 'get(v:, "val", -1)')
echo map({'a': 1}, 'v:["key"]')
echo map([[1, 2]], 'map(copy(v:val), "v:val * 10") + [v:val]')
echo map({'k': [3]}, 'map(copy(v:val), "v:key") + [v:key]')
call map([9], '1')
try
  echo v:val
catch
  echo v:exception
endtry
call filter([1], 'v:key >= 0')
try
  echo v:key
catch
  echo v:exception
endtry
