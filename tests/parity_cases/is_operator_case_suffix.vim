" get_compare reads `is`/`isnot` with a `#` or `?` suffix, as for `==#`/`==?`.
echo 'a' is? 'A' 'a' is# 'A' 'a' isnot? 'A' 'a' isnot# 'a' [1] is# [1] 1 is#1
let l = [1]
echo l is# l l isnot? l
set ic
echo 'a' is 'A' 'a' is# 'A' 'a' isnot 'A'
set noic
echo 'a' is 'A'
