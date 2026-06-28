" editor_absent.vim — builtins whose answer is well-defined when running
" outside an editor: fold-close queries (fold.c), mapping queries (mapping.c),
" and the deprecated buffer/file aliases (deprecated.c → bufexists/bufname/
" bufnr/filereadable). Self-test: asserts into v:errors, throws if any failed.

" --- no folds: every line reports no closed fold
call assert_equal(-1, foldclosed(1))
call assert_equal(-1, foldclosed(999))
call assert_equal(-1, foldclosedend(1))

" --- no mappings: the mapping queries report nothing
call assert_equal(0, hasmapto('<C-x>'))
call assert_equal('', maparg('<F2>'))
call assert_equal('', mapcheck('<F2>'))
call assert_equal([], maplist())

" --- maparg() with a truthy {dict} argument returns an (empty) Dict, not ''
call assert_equal({}, maparg('<F2>', 'n', 0, 1))

" --- deprecated aliases resolve to their modern counterparts
call assert_equal(bufexists('%'), buffer_exists('%'))
call assert_equal(bufname('%'), buffer_name('%'))
call assert_equal(bufnr('%'), buffer_number('%'))
call assert_equal(filereadable('/etc/hosts'), file_readable('/etc/hosts'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'editor_absent.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'editor_absent.vim: all assertions passed'
