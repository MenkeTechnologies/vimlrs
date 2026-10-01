" :set on a number option: += adds, -= subtracts, ^= multiplies.
set tw+=10
echo &tw
set tw-=3
echo &tw
set tw^=2
echo &tw
set ts+=1
echo &ts
set sw=4 sw+=2
echo &sw
set sts^=3
echo &sts
set tw&
echo &tw
let &tw += 5
echo &tw
