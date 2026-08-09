" matchadd()/matchaddpos() auto-assign ids from a per-window counter that is READ
" and then incremented, so the FIRST auto id is 1000, not 1001 (this port
" pre-incremented), and an explicit {id} never advances the counter. `:help
" matchadd()` only promises "a free ID, which is at least 1000"; every row below
" is the behaviour vim 9.2 actually prints, recorded in the .expected beside this
" file. The C that implements it (`match_add`, window.c) is NOT part of the
" vendored Neovim subset — vendor/window.c carries only find_tabpage and
" win_get_tabwin — so this case, not a C citation, is the pin.
"
" The match list is also kept in ascending priority order rather than insertion
" order: an equal priority appends after its peers and a higher one sinks to the
" end regardless of when it was added. This port appended everything.
echo matchadd('Search', 'a')
echo matchadd('Search', 'b')
echo matchadd('Search', 'c', 20, 42)
echo matchadd('Search', 'd')
echo string(map(getmatches(), {_, v -> [v.id, v.priority]}))
call clearmatches()
" clearmatches() empties the list but does not rewind the id counter.
echo matchadd('Search', 'e')
echo matchaddpos('Search', [1])
echo matchadd('Search', 'f', 5)
echo matchaddpos('Search', [2], 30)
echo matchadd('Search', 'g', 5)
echo string(map(getmatches(), {_, v -> [v.id, v.priority]}))
echo matchdelete(1004)
echo string(map(getmatches(), {_, v -> v.id}))
echo matchadd('Search', 'h')
echo string(map(getmatches(), {_, v -> v.id}))
" setmatches() re-adds every entry, so the rebuilt list comes back in ascending
" priority order rather than the order it was handed, and the ids it restores
" still do not advance the counter. vim IS the oracle for these rows: `Search` is
" a default highlight group, so `-u NONE` resolves it and no E28 is raised.
call clearmatches()
call setmatches([{'group': 'Search', 'pattern': 'p', 'priority': 50, 'id': 7}, {'group': 'Search', 'pattern': 'q', 'priority': 1, 'id': 8}])
echo string(map(getmatches(), {_, v -> [v.id, v.priority]}))
echo matchadd('Search', 'r')
echo string(map(getmatches(), {_, v -> [v.id, v.priority]}))
" An equal-priority entry restored by setmatches() still lands after its peers.
call setmatches([{'group': 'Search', 'pattern': 's', 'priority': 10, 'id': 3}, {'group': 'Search', 'pattern': 't', 'priority': 2, 'id': 4}, {'group': 'Search', 'pattern': 'u', 'priority': 10, 'id': 5}])
echo string(map(getmatches(), {_, v -> [v.id, v.priority]}))
