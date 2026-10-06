" printf(): '%' takes the width and the '-'/'0' flags like %c (vim_vsnprintf_typval),
" and the E766/E767 wording.
echo printf('[%5%][%-5%][%05%][%.0%][%*%]', 3)
echo printf('[%5%]', 1)
echo printf('[%1$5%]')
echo printf('%d %d', 1)
echo 'a'
echo printf('%d', 1, 2)
echo 'b'
echo printf('%1$d %d', 1, 2)
echo 'c'
echo printf('%d', 1, 2) 'x'
