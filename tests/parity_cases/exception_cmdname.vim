" An error turned into an exception is tagged with the command that raised
" it (`Vim(delfunction):…`); commands missing from the tag table used to keep
" the tag of the command before them (`Vim(try):…`).
try
  delfunc Nope
catch
  echo v:exception
endtry
for cmd in ['delfunc Nope2', 'delfunction Nope3']
  try
    exe cmd
  catch
    echo v:exception
  endtry
endfor
