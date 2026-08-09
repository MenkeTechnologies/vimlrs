" v:none prints under its own name, not v:null — which also decides how the two
" compare, because typval_compare() falls through to a string compare for two
" VAR_SPECIALs.
echo string(v:none)
echo string(v:null)
echo v:none .. '-2'
echo repeat(v:none, 3)
echo v:null == v:none
echo v:null != v:none
echo v:none is v:null
echo v:none isnot v:null
echo v:none == v:none
echo v:null == v:null
echo 'x' .. v:none .. v:null
echo string([v:none, v:null])
echo empty(v:none)
echo type(v:none)
echo typename(v:none)
