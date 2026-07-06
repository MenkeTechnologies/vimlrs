vim9script
# vim9 predefined `null_*` constants (see `:help vim9.txt`, "Predefined
# variables"). Each is the null value of its type; oracle-verified against
# /opt/homebrew/bin/vim 9.2 (Vim 9.2.0750). Real vim runtime file
# autoload/dist/vim9.vim uses `null_string` (`var os_viewer = null_string`)
# and failed to load in vimlrs with E121 before this was ported.

# null_string: a null String — type 1, stringifies to '', equals ''.
assert_equal(1, type(null_string))
assert_equal("''", string(null_string))
assert_true(null_string == '')

# null_list: a null List — type 3, stringifies to '[]'.
assert_equal(3, type(null_list))
assert_equal('[]', string(null_list))

# null_dict: a null Dict — type 4, stringifies to '{}'.
assert_equal(4, type(null_dict))
assert_equal('{}', string(null_dict))

# null_blob: a null Blob — type 10, stringifies to '0z'.
assert_equal(10, type(null_blob))
assert_equal('0z', string(null_blob))

# null_function / null_partial: a null Funcref — type 2, stringifies to
# function('').
assert_equal(2, type(null_function))
assert_equal("function('')", string(null_function))
assert_equal(2, type(null_partial))
assert_equal("function('')", string(null_partial))

if len(v:errors) > 0
  echoerr 'vim9_null_constants FAILED: ' .. string(v:errors)
endif
