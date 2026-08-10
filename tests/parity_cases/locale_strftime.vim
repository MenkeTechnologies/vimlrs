" `strftime()` must not change its answer because something else ran first.
"
" `init_locale()` (setlocale(LC_ALL, "")) was reached from ONE place: the
" `strcoll` branch of `item_compare()`, i.e. `sort(…, 'l')`. Every other
" locale-dependent libc call ran in the process's default "C" locale until a
" locale-collating sort happened to occur, so this script printed 0 for the
" first comparison below and 1 for the second — a `strftime()` result that
" depended on whether a `sort()` earlier in the same file used the 'l' flag.
" Under TZ=UTC LC_ALL=de_DE.UTF-8 the two readings were literally
" `01/01/70 Thursday` and `01.01.1970 Donnerstag`. vim calls `init_locale()`
" from main(), so both of its readings are the same and both are German.
"
" Nothing here pins a locale-DERIVED string, deliberately: neither harness sets
" LC_ALL/LANG/LC_TIME, so a record of `01/01/1970` or `Donnerstag` would be a
" record of the machine that ran the recorder. What is pinned is the INVARIANT —
" that the same call answers the same thing throughout one script, whatever the
" ambient locale is. This case was verified byte-identical between vim 9.2.0900
" and viml under LC_ALL=C, en_US.UTF-8, de_DE.UTF-8 and fr_FR.UTF-8.

let s:fmt = '%x %X %A %B %p %c'
let s:before = strftime(s:fmt, 0)

" The one call that used to install the locale as a side effect.
call sort(['b', 'a', 'ä', 'A'], 'l')
let s:after = strftime(s:fmt, 0)
echo 'strftime stable across sort(l):' (s:before ==# s:after)

" Two more locale-sensitive readers, same question.
let s:p1 = strptime('%d %B %Y', '01 January 1970')
call sort(['z', 'y'], 'l')
echo 'strptime stable across sort(l):' (s:p1 ==# strptime('%d %B %Y', '01 January 1970'))

" And the reverse order: a locale reader FIRST, then the sort, then the reader.
" Both engines must be stable in either direction.
echo 'strftime stable in both directions:' (strftime(s:fmt, 0) ==# s:before)

" LC_NUMERIC is forced back to "C" by init_locale(), so adopting the locale must
" NOT move the decimal point — this is the regression the C guards against with
" its own `setlocale(LC_NUMERIC, "C")` right after `setlocale(LC_ALL, "")`.
echo printf('%f %g %e %.2f', 1.5, 1234567.5, 1.5, 3.14159)
echo string(1.5) str2float('1.5') str2float('1,5') str2nr('12')

" A locale-collating sort still collates, and an ASCII-only case is the same
" answer in every locale (BUGS.md R14 chose this shape for the fuzz corpus for
" the same reason).
echo string(sort(['b', 'a', 'c'], 'l'))
echo string(sort(['b', 'A', 'a', 'B'], 'i'))
echo string(sort([10, 9, 100], 'n'))
