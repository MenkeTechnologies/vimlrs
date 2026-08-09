" list2str() encodes each code point with utf_char2bytes and appends it with the
" STRLEN-based ga_concat, so a 0 contributes nothing AND the walk continues to
" the next item. It does not terminate the string.
echo string(list2str([65, 0, 66]))
echo string(list2str([104, 0, 105, 0]))
echo string(list2str([0, 0, 0]))
echo string(list2str([65, 0, 66], 1))
echo string(list2str([65, 66, 67]))
echo string(list2str([]))
echo len(list2str([65, 0, 66]))
