" undo.vim — undofile()/undotree(), ported from Neovim's undo.c. Standalone
" there is no undo history, but undofile() still computes the undo-file path.
" Self-test: asserts into v:errors, throws at the end if anything failed.

" --- undofile() puts '.{name}.un~' next to the file (default 'undodir' of '.')
call assert_equal('/tmp/.notes.txt.un~', undofile('/tmp/notes.txt'))
call assert_equal('/a/b/.x.un~', undofile('/a/b/x'))
call assert_equal('.notes.txt.un~', undofile('notes.txt'))

" --- an empty name yields ''
call assert_equal('', undofile(''))

" --- undotree() reports an empty, synced tree standalone
let t = undotree()
call assert_equal(0, t.seq_last)
call assert_equal(0, t.seq_cur)
call assert_equal(0, t.save_last)
call assert_equal(1, t.synced)
call assert_equal([], t.entries)

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'undo.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'undo.vim: all assertions passed'
