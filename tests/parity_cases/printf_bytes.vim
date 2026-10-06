" printf() renders %c/%s/%S as BYTES (strings.c vim_vsnprintf_typval):
" %c is one byte (the value truncated), %.Ns cuts at a byte, a non-UTF-8 %s
" argument passes through, the 0 flag pads %s/%S/%c with zeros, and a NUL
" from %c ends the result (f_printf stores a C string).
echo len(printf('%c', 233)) printf('%c', 233) == "\xe9"
echo len(printf('%c', 0x141)) printf('%c', 0x141)
echo len(printf('%3c|', 0)) len(printf('ab%cde', 256))
echo len(printf('%-3c|', 233))
echo printf('%c%c', 0xc3, 0xa9)
echo printf('%05s|%05S|%05c|%-05s|', 'ab', 'é', 65, 'x')
echo printf('%.2s|', 'héllo') == "h\xc3|"
echo printf('%s', "\xe9") == "\xe9" printf('%5s|', "\xe9") == "    \xe9|"
echo printf('%.1S|%3S|%5.2s|', 'éa', 'é', 'abc')
