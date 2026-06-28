" json.vim — json_encode()/json_decode() round-trip, with embedded unit tests.
"
" Demonstrates: building a nested Dict/List, encoding to JSON, decoding back,
" reading fields, and asserting the round-trip is lossless. Exits non-zero on
" failure.
"
"   vimlrs examples/json.vim

let config = {'name': 'vimlrs', 'version': [0, 1, 0], 'features': ['jit', 'lsp', 'dap'], 'standalone': v:true}

let encoded = json_encode(config)
let decoded = json_decode(encoded)

" ── unit tests ──
call assert_equal('{"name":"vimlrs","version":[0,1,0],"features":["jit","lsp","dap"],"standalone":true}', encoded)
call assert_equal('vimlrs', decoded['name'])
call assert_equal([0, 1, 0], decoded['version'])
call assert_equal('0.1.0', join(map(copy(decoded['version']), 'string(v:val)'), '.'))
call assert_equal(['jit', 'lsp', 'dap'], decoded['features'])
call assert_true(index(decoded['features'], 'jit') >= 0)
call assert_equal(config, decoded)

" ── demo ──
echo 'encoded :' encoded
echo 'name    :' decoded['name']
echo 'features:' decoded['features']

" ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) . ' assertion(s) failed'
endif
echo 'OK: json assertions passed'
