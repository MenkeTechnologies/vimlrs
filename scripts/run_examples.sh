#!/bin/sh
# run_examples.sh — run every examples/*.vim through the vimlrs binary and fail
# if any exits non-zero. Each example self-checks with the built-in assert_*
# framework and `throw`s (non-zero exit) on a failed assertion, so this is the
# example-script regression gate used by CI (the `examples` job).
#
# A tests/fixtures/<name>.in file, when present, is piped to the script's stdin
# (the interactive example). Otherwise stdin is empty (EOF).
#
# Binary resolution: $VIMLRS, else target/release/vimlrs, else target/debug/vimlrs.
#   sh scripts/run_examples.sh
#   VIMLRS=/path/to/vimlrs sh scripts/run_examples.sh
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BIN="${VIMLRS:-}"
if [ -z "$BIN" ]; then
  if [ -x target/release/vimlrs ]; then
    BIN=target/release/vimlrs
  elif [ -x target/debug/vimlrs ]; then
    BIN=target/debug/vimlrs
  else
    echo "no vimlrs binary found — build first (cargo build --release)" >&2
    exit 2
  fi
fi
echo "running examples with: $BIN"

fail=0
total=0
for f in examples/*.vim; do
  total=$((total + 1))
  stem="$(basename "$f" .vim)"
  infile="tests/fixtures/$stem.in"
  [ -f "$infile" ] || infile=/dev/null

  if "$BIN" "$f" <"$infile" >/dev/null 2>&1; then
    echo "ok   $stem"
  else
    echo "FAIL $stem (exit $?)"
    # Re-run showing output so the CI log captures the failure detail.
    "$BIN" "$f" <"$infile" 2>&1 | sed 's/^/     | /'
    fail=$((fail + 1))
  fi
done

echo "---"
echo "$((total - fail))/$total example scripts passed"
[ "$fail" -eq 0 ] || exit 1
