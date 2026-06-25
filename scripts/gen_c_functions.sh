#!/bin/sh
# Extract every function-like identifier appearing in the vendored Neovim eval
# C source (csrc/). A VimL port fn name is "legal" if it appears here (i.e.
# exists in upstream C as a callable) — definitions, calls, and macro-generated
# names alike. Regenerate after vendoring more C:  sh scripts/gen_c_functions.sh
set -e
cd "$(dirname "$0")/.."
grep -rhoE '\b[a-zA-Z_][a-zA-Z0-9_]*[[:space:]]*\(' csrc \
    --include='*.c' --include='*.h' --include='*.lua' 2>/dev/null \
  | sed -E 's/[[:space:]]*\($//' \
  | sort -u > docs/nvim_c_functions.txt
echo "$(wc -l < docs/nvim_c_functions.txt) names -> docs/nvim_c_functions.txt"
