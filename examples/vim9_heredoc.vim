vim9script
# vim9_heredoc.vim — vim9 `var/final/const X =<< [trim] [eval] END` here-doc.
#
# In vim9 script the declaration keywords `var`, `final` and `const` open a
# here-document with the same `=<<` list-assignment form as legacy `:let`
# (`:help vim9`). The lines that follow, up to a line equal to MARKER, become a
# List of strings; `trim` strips the first body line's indent from every line.
# Self-tests into v:errors; exits non-zero on failure.
#
#   vimlrs examples/vim9_heredoc.vim

# ── typed `var` heredoc → List of lines ──
var basic: list<string> =<< END
hello
world
END
assert_equal(['hello', 'world'], basic)

# ── untyped `var` heredoc ──
var untyped =<< END
alpha
beta
END
assert_equal(['alpha', 'beta'], untyped)

# ── `final` heredoc with `trim`: first body line's indent removed from every
#    line; deeper relative indent preserved ──
final trimmed: list<string> =<< trim END
    root
      child
    root2
END
assert_equal(['root', '  child', 'root2'], trimmed)

# ── body is verbatim: quotes, bars and `#` are literal, not comments ──
var raw =<< EOF
it's got a | pipe
a "quoted" word
# not a comment here
EOF
assert_equal(["it's got a | pipe", 'a "quoted" word', '# not a comment here'], raw)

# ── empty heredoc → empty List ──
var none =<< END
END
assert_equal([], none)

# ── composes with normal code on either side ──
var before = 1
var payload =<< END
data
END
var after = 2
assert_equal(1, before)
assert_equal(2, after)
assert_equal(['data'], payload)

# ── self-test epilogue ──
if !empty(v:errors)
  for e in v:errors
    echo 'FAIL:' e
  endfor
  throw len(v:errors) .. ' assertion(s) failed'
endif
echo 'OK: vim9 heredoc assertions passed'
