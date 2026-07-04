" sha256_digest.vim — the sha256() builtin (crypt/sha256.c). Returns the lower-
" case hex SHA-256 digest of the UTF-8 bytes of its String argument: always 64
" hex characters, deterministic, and matching the published FIPS-180-4 vectors.
" Multibyte input is hashed over its UTF-8 encoding, so 'café' differs from an
" ASCII string. Digests below are cross-checked against nvim/vim's sha256().
" Self-test: asserts into v:errors, throws if any failed.

" --- the canonical empty-string and 'abc' FIPS test vectors
call assert_equal('e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855', sha256(''))
call assert_equal('ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad', sha256('abc'))

" --- a longer ASCII phrase
call assert_equal('5cac4f980fedc3d3f1f99b4be3472c9b30d56523e632d151237ec9309048bda9', sha256('The quick brown fox'))

" --- multibyte input hashes its UTF-8 bytes (é is two bytes)
call assert_equal('850f7dc43910ff890f8879c0ed26fe697c93a067ad93a7d50f466a7028a9bf4e', sha256('café'))

" --- a 1000-'a' input exercises multi-block compression
call assert_equal('41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3', sha256(repeat('a', 1000)))

" --- structural invariants: exactly 64 chars, all lowercase hex, deterministic
call assert_equal(64, strlen(sha256('anything')))
call assert_equal(64, strlen(sha256('')))
call assert_equal(sha256('repeatable'), sha256('repeatable'))
" digest is already lowercase hex, so tolower() is a no-op on it
call assert_equal(sha256('x'), tolower(sha256('x')))

" --- a single flipped input bit produces a wholly different digest
call assert_equal(0, sha256('a') == sha256('b'))
call assert_equal(1, sha256('a') != sha256('A'))

if len(v:errors) > 0
  for err in v:errors
    echo err
  endfor
  throw 'sha256_digest.vim: ' . len(v:errors) . ' assertion(s) failed'
endif
echo 'sha256_digest.vim: all assertions passed'
