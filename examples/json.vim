" json.vim — round-trip data through json_encode()/json_decode().
"
" Demonstrates: building a nested Dict/List, encoding it to JSON, decoding it
" back, and reading fields out of the decoded structure.
"
"   vimlrs examples/json.vim

let config = {'name': 'vimlrs', 'version': [0, 1, 0], 'features': ['jit', 'lsp', 'dap'], 'standalone': v:true}

let encoded = json_encode(config)
echo 'encoded :' encoded

let decoded = json_decode(encoded)
echo 'name    :' decoded['name']
echo 'version :' join(map(copy(decoded['version']), 'string(v:val)'), '.')
echo 'features:' decoded['features']
echo 'jit?    :' (index(decoded['features'], 'jit') >= 0)
