" A number option out of range is reported by its did_set_* check and replaced
" (vim stores the corrected value, not the old one), through :let &opt and :set.
set ts=4
let &ts = -1
echo &ts
let &ts = 0
let &ts = 10000
echo &ts
set tw=5
let &tw = -1
echo &tw
set sw=3
let &sw = -1
echo &sw
let &report = -1
echo &report
let &ch = 0
echo &ch
let &wh = 0
echo &wh
set wh=5
let &wmh = 7
echo &wmh &wh
let &wiw = 0
let &hh = -1
let &nuw = 0
let &nuw = 21
echo &nuw
let &cole = 4
echo &cole
let &imi = 3
let &ims = 3
let &re = 3
let &ut = -1
echo &ut
let &tm = -1
let &uc = -1
echo &uc
let &cwh = 0
let &fdl = -3
echo &fdl
let &ul = -5
echo &ul
