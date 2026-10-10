" A word that names no Ex command is E492 whether or not anything follows it —
" legacy script has no bare-expression statement. The message quotes the command
" as written (blanks, modifiers, and the rest of the line after a `|`), and the
" exception it becomes carries no `(cmd)` tag.
nosuchcmd
nosuchcmd arg
foo = 1
   nosuchcmd
keepjumps   nosuchcmd arg
echo 1 | nosuchcmd | echo 2
echo 'next'
echo execute('nosuchcmd')
echo execute("  nosuchcmd x ")
try
  nosuchcmd
catch
  echo v:exception
endtry
try | execute 'nosuchcmd' | catch | echo v:exception | endtry
try | call execute('nosuchcmd') | catch | echo v:exception | endtry
silent! nosuchcmd
if 0
  nosuchcmd
endif
function! F()
	 nosuchcmd x
  echo 'continues'
endfunction
call F()
Nosuchcmd x | echo 'swallowed'
try
  echo [][1]
catch
  echo v:exception
  nosuchcmd
endtry
echo "not reached"
