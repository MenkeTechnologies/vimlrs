" Vim-only base64_*, blob2str()/str2blob() and uri_* builtins. base64 pads only
" at the end of the whole input, so an inner "=" decodes as zero bits.
for e in ["base64_encode(0z)", "base64_encode(0z61)", "base64_encode(0z6162)",
      \ "base64_encode(0z000102ff)", "base64_encode('abc')", "base64_decode('YQ==')",
      \ "base64_decode('YWJjZGU=')", "base64_decode('YQ==YQ==')", "base64_decode('YQ=a')",
      \ "base64_decode('YQ')", "base64_decode('YR==')", "base64_decode('!!!!')",
      \ "base64_decode(0z61)",
      \ "blob2str(0z)", "blob2str(0z610a62)", "blob2str(0z610062)", "blob2str(0z610a)",
      \ "blob2str(0z0a0a)", "blob2str(0zff)", "blob2str(0zc3a9)",
      \ "blob2str(0ze9, {'encoding':'latin1'})", "blob2str(0zff, {'encoding':'none'})",
      \ "blob2str(0z61, {'encoding':'bogus'})", "blob2str(0z61, 1)", "blob2str('a')",
      \ "str2blob([])", "str2blob([''])", "str2blob(['', ''])", "str2blob(['a', 'b'])",
      \ "str2blob(['a', 1, 'b'])", "str2blob([1, 'a'])", "str2blob([\"a\\nb\"])",
      \ "str2blob(['é'])", "str2blob(['é'], {'encoding':'latin1'})",
      \ "str2blob(['a'], {'encoding':'bogus'})", "str2blob('a')",
      \ "uri_encode('abc-._~')", "uri_encode('a b/c?d=e&f')", "uri_encode('é')",
      \ "uri_encode(1)", "uri_decode('%41%42')", "uri_decode('%c3%a9')", "uri_decode('a+b')",
      \ "uri_decode('%')", "uri_decode('%4')", "uri_decode('%zz')", "uri_decode('a%2')",
      \ "uri_decode(1)", "base64_encode()", "uri_encode('a', 'b')"]
  try
    echo e '=' string(eval(e))
  catch
    echo e '!' v:exception
  endtry
endfor
