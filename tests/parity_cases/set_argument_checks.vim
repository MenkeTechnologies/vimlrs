" :set follows do_set_option(): E474 for a value on a boolean or a prefix on a
" number, E488 for trailing text (`ic&x`), E521 for a value that is not a number (octal
" and hex are), E487/E474 from the option's own check. The first error ends the
" command and quotes the argument up to the next one.
set ts=abc
set ts=4x
set ts=
set ts+=abc
set ic=1
set noic=1
set nots
set invts
set ic&x
set ts=0
set tw=-1
set ts=10000
set wc=13
set ts=8 tw=nope sw=3
echo &ts &tw &sw
set ts=010
echo &ts
set ts=0x10
echo &ts
set ts:5
echo &ts
set ic noic
echo &ic
set ic! ts=4
echo &ic &ts
