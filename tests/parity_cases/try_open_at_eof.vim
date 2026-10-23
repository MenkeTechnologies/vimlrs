" An unclosed :try at the end of a sourced file is E600 after its lines ran.
echo 'before'
try
  echo 'in the try'
catch
  echo 'not reached'
