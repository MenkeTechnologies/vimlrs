vim9script
# get_var_special_name() (evalvars.c:2676): vim9 code names Bool and null
# values without the `v:` prefix; v:none keeps it; legacy code keeps all three.
echo true false null v:none
echo [true, null] {a: false}
echo string(true) string([null, false, v:none])
echo true .. '|' .. null
echo printf('%s %S', false, [true])
# json_encode() has its own names and is unaffected.
echo json_encode([true, false, null])
# execute() runs the text in the caller's (vim9) context.
echo execute('echo [true, null]')
# A legacy :function in a vim9 file runs as legacy (userfunc.c:5757).
function Legacy()
  return string([v:true, v:false, v:null])
endfunction
echo Legacy()
def Vim9(): string
  return string([true]) .. Legacy() .. string(false)
enddef
echo Vim9()
# Back at script level after the calls: still vim9.
echo [false]
