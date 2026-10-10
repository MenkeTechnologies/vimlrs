" A missing :throw argument is E471 — with the command appended only when
" nothing follows it on the line. A non-String argument is converted AFTER it
" evaluated, so its conversion error does not stop the (empty) exception being
" thrown: the message prints, then E605 reports the exception uncaught.
echo 'start'
throw
  throw  
throw | echo 'after bar'
echo 'before'
throw {}
echo 'not reached'
