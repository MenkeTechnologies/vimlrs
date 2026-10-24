" A :def body runs as vim9 even in a legacy script (vim9execute.c:6903), so it
" names Bool/null values `true`/`false`/`null`; the legacy script around it
" keeps `v:true`/`v:false`/`v:null`.
def D(): string
  echo true null [true, false] {a: null}
  return string([false])
enddef
let s:r = D()
echo s:r
echo v:true v:false v:null string([v:true, v:none])
echo v:true . ''
function L()
  return D() . string(v:false)
endfunction
echo L()
echo execute('echo v:null')
" A sourced vim9 file switches only itself (do_source() saves current_sctx).
let s:f = tempname()
call writefile(['vim9script', 'echo [true, null]'], s:f)
execute 'source' s:f
call delete(s:f)
echo [v:true, v:null]
