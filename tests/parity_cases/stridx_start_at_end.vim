" A {start} at or past the end of the haystack is -1 even for an empty needle
" (`if (error || start_idx >= STRLEN(haystack)) return;`); without {start} the
" empty needle matches at 0.
echo stridx('abc', '', 3)
echo stridx('abc', '', 2)
echo stridx('abc', '', 5)
echo stridx('', '', 0)
echo stridx('', '', 1)
echo stridx('', '')
echo stridx('abc', '')
echo stridx('abc', 'c', 2)
echo stridx('abc', 'c', 3)
echo stridx('abc', 'a', -5)
echo stridx('abc', '', -1)
