" In silent Ex mode a message is written only at 'verbose' > 0 (c:
" msg_puts_printf): with 'verbose' 0 neither :echo nor an error shows, while a
" :set opt? value, which showoneopt() writes with silent_mode off, still does.
echo 'shown'
set vbs=0
echo 'x'
echo [][0]
echoerr 'boo'
set ts?
set vbs=1
echo 'y'
