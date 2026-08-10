" `var_check_ro()` (`vendor/eval/vars.c:2947`): a VV_RO `v:` slot REPORTS
" `E46: Cannot change read-only variable "%.*s"` and keeps its value. This port
" declined the assignment in silence, so nothing distinguished a read-only slot
" from a writable one.
"
" The values are not echoed — `v:version` is 801 here (Neovim's, the porting
" spec) and 902 in vim — so each probe pins the MESSAGE and that the value did
" not move.
let s:v = v:version
try | let v:version = 12345 | catch | echo v:exception | endtry
echo 'unchanged:' (v:version == s:v)

let s:v = v:t_number
try | let v:t_number = 12345 | catch | echo v:exception | endtry
echo 'unchanged:' (v:t_number == s:v)

try | let v:t_string = 12345 | catch | echo v:exception | endtry
try | let v:t_list = 12345 | catch | echo v:exception | endtry
try | let v:count = 12345 | catch | echo v:exception | endtry
try | let v:count1 = 12345 | catch | echo v:exception | endtry
try | let v:shell_error = 12345 | catch | echo v:exception | endtry
try | let v:numbermax = 12345 | catch | echo v:exception | endtry
try | let v:numbersize = 12345 | catch | echo v:exception | endtry
try | let v:true = 12345 | catch | echo v:exception | endtry
" (v:count1 is not echoed: it is 1 in Neovim — the porting spec, and what this
" engine answers — and 0 in vim under -es.)
echo v:t_number v:t_string v:t_list v:count v:shell_error v:numbersize v:true

" A slot that is read-only only INSIDE the sandbox is writable outside one
" (`c:2953` gates VV_RO_SBX on `sandbox`).
let v:lnum = 7
echo v:lnum

" A mutable slot round-trips.
let v:errmsg = 'boom'
echo v:errmsg

" An unknown v: name cannot be created at all.
try | let v:no_such_vimvar_xyz = 1 | catch | echo v:exception | endtry
