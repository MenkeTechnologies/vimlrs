" json_encode(): vim writes NaN/Infinity, joins Blob bytes with a bare comma,
" and refuses a Funcref with E1161 (the whole result is then '').
echo json_encode(1.0 / 0) json_encode(-1.0 / 0) json_encode(0.0 / 0)
echo json_encode([1.5, 1.0 / 0])
echo json_encode(0z0102) json_encode(0z) json_encode({'b': 0z00ff})
echo json_encode([function('len')]) 'after'
echo json_encode({'a': {-> 1}}) 'after'
echo json_encode(function('len', [1])) 'after'
