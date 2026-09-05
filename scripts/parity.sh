#!/usr/bin/env bash
# parity.sh — script-level differential harness: vimlrs vs the real vim binary.
#
# The existing `cargo run --bin fuzz-parity` fuzzes *expressions* and single
# statements through `execute()`. This harness works one level up: it sources a
# whole `.vim` file through BOTH interpreters and byte-diffs the captured output
# and the process exit status. That is the only way to see the divergences that
# live between statements rather than inside one — the message-column model
# (`:echo` vs `:echon` vs `echo ''`), option state (`:set ignorecase`) leaking
# into later comparisons, a `:const`/`:function d.key()` that parses but leaves
# nothing behind, and an error aborting the wrong amount of the script.
#
#   bash scripts/parity.sh                        # run the committed corpus
#   bash scripts/parity.sh tests/parity_cases     # ... explicitly
#   bash scripts/parity.sh probe.vim              # one ad-hoc case, verbose
#   bash scripts/parity.sh -r tests/parity_cases  # re-record .expected from vim
#   bash scripts/parity.sh -q                     # summary only
#
#   -r DIR   re-record every case's `.expected` from a fresh run of *vim*.
#            Only vim writes an expectation — never vimlrs, so a case can never
#            be made to pass by changing vimlrs.
#   -q       summary only
#   -h       this message
#
# How vim is driven, and why:
#
#   vim -es -u NONE -i NONE -N -c 'verbose source FILE' -c 'qa!'
#
# `-es` is silent Ex mode (no terminal escapes, no alternate screen). In that
# mode `:echo` output is suppressed *unless* the sourcing command is `verbose`,
# and `-S FILE` is not equivalent to `-c 'verbose source FILE'` — it prints
# nothing at all. Both were verified before this harness was written; changing
# either flag silently turns every case into "both printed nothing".
#
# `-N` is `nocompatible`. Without it, `-u NONE` leaves vim in COMPATIBLE mode,
# which is not the dialect this crate ports: the engine is Neovim-derived and
# Neovim has no compatible mode at all. Fourteen observables move between the
# two states (`&compatible`, `&cpoptions`, `&fileformats`, `&backspace`,
# `&whichwrap`, `&history`, `&viminfo`, `&formatoptions`, `&modeline`,
# `&shortmess`, `&more`, `&esckeys`, `&ruler`, `&showcmd`), and every one of
# them was invisible to this harness by construction while the flag was absent.
# `-i NONE` stays: a nocompatible vim without it reads AND WRITES `~/.viminfo`,
# so registers and histories would leak in from the developer's disk and one
# probe would mutate what the next one sees.
#
# Two normalisations are applied to vim's stream, and only these two:
#
#   1. CR is stripped. Silent Ex mode still writes CRLF because it thinks it is
#      driving a terminal; that is a tty artifact, not language behaviour.
#   2. vim's error *preamble* lines are dropped — `Error detected while
#      processing …:` (which embeds the case's absolute path, so it can never
#      match) and the `line    N:` that follows it. The `E123: …` message itself
#      is compared verbatim, because that is the part vimlrs promises.
#
# Nothing else is filtered, and there is no allowlist: a divergence either shows
# up in the report or the harness is broken.
set -uo pipefail
cd "$(dirname "$0")/.."

# The oracle is ONE ABSOLUTE BINARY, resolved once. `command -v vim` re-resolves
# PATH on every call, so a `brew upgrade` (or a shell that rewrites PATH between
# the record pass and the check pass) would silently swap reference editors
# mid-run and report the difference as a vimlrs divergence.
VIM="${VIM:-$(command -v vim || true)}"
if [ -n "$VIM" ]; then
  # `realpath` is not on every macOS; perl's Cwd is.
  VIM_ABS=$(perl -MCwd=abs_path -e 'print abs_path($ARGV[0])' "$VIM" 2>/dev/null || printf '%s' "$VIM")
  [ -n "$VIM_ABS" ] && VIM="$VIM_ABS"
fi
VIML="${VIML:-}"
QUIET=0
RERECORD=

while [ $# -gt 0 ]; do
  case "$1" in
    -r|--rerecord) RERECORD="$2"; shift 2 ;;
    -q|--quiet)    QUIET=1; shift ;;
    -h|--help)     grep '^#' "$0" | cut -c3-; exit 0 ;;
    -*)            echo "unknown flag: $1" >&2; exit 2 ;;
    *)             break ;;
  esac
done

TARGET="${1:-tests/parity_cases}"

[ -n "$VIM" ] || { echo "no vim on PATH — this harness needs the reference editor as ground truth" >&2; exit 2; }
if [ -z "$VIML" ]; then
  if   [ -x target/debug/viml ];   then VIML=target/debug/viml
  elif [ -x target/release/viml ]; then VIML=target/release/viml
  else echo "no viml binary — run \`cargo build' first" >&2; exit 2; fi
fi

if [ -t 1 ]; then C='\033[36m'; G='\033[32m'; R='\033[31m'; D='\033[2m'; N='\033[0m'
else C= G= R= D= N=; fi
say() { printf "${C}==>${N} %s\n" "$1"; }

OUT=target/parity
rm -rf "$OUT"; mkdir -p "$OUT"

# Drop vim's error preamble and its CRs. See the header for why only these two.
#
# Both are done in ONE perl pass, on bytes. `tr -d '\r'` cannot do this job: in a
# UTF-8 locale macOS `tr` aborts with "Illegal byte sequence" the moment vim
# writes a byte that is not valid UTF-8, TRUNCATING the rest of the stream — and
# vim writes such bytes for real (`list2str([-1])` is the single byte 0xff). A
# case like that would have had its `.expected` silently recorded short. perl
# without `use utf8` is byte-transparent and passes them through untouched.
norm() { perl -ne 's/\r//g; next if /^Error detected while processing /; next if /^line\s+\d+:$/; print'; }

# The environment both engines run in, pinned. Without this the record depends
# on the shell of whoever re-records it. Measured on the 41-case corpus, by
# replaying it through vim under different ambient settings and diffing against
# the committed records:
#
#   LC_ALL=en_US.UTF-8   0/41 move   (what the records were taken at)
#   LC_ALL=C             9/41 move
#   LC_ALL=de_DE.UTF-8  13/41 move
#   LC_ALL=tr_TR.UTF-8  13/41 move
#
# Each variable, and the specific thing it moves:
#
#   LC_CTYPE   decides `&encoding`. Any UTF-8 locale gives `utf-8`; `C`/`POSIX`
#              gives `latin1`, which changes every byte-level answer in the
#              corpus — `list2str_bytes`, `list2str_nul`, `echo_transchar`,
#              `match_start_bytes`, `regex_composing_start`, `case_mapping`,
#              `option_exists`, `string_builtins`, `cellwidths_table`. That is
#              the 9.
#   LC_MESSAGES vim ships translated diagnostics in `$VIMRUNTIME/lang`, and this
#              port never translates. Under de_DE every `E<n>:` text becomes
#              German. That is the 13.
#   LANGUAGE   gettext reads LANGUAGE *first* and it is a colon-separated list
#              rather than a locale name, so setting LC_ALL alone does NOT
#              override it. Cleared, not set.
#   TZ         `strftime()`/`localtime()` answers.
#
# `C.UTF-8` is the pinned locale rather than `en_US.UTF-8` because it is the one
# UTF-8 locale present without having to be generated (glibc >= 2.35, musl, and
# macOS all carry it), and it gives `&encoding=utf-8` with untranslated
# messages. Verified: re-recording the whole corpus under it changes no record.
#
# `VIM` and `VIMRUNTIME` are UNSET for the child, which is not cosmetic: they are
# how vim locates its runtime, and this script's own binary-selection variable is
# also named `VIM`. A developer who exports `VIM` (a common habit) hands vim a
# path that is not a runtime directory; vim then fails to find
# `$VIMRUNTIME/lang` and silently stops translating — which looks exactly like a
# correctly pinned locale while actually being a broken reference editor. That
# was measured, not assumed: exporting `VIM=<path to the binary>` took the de_DE
# figure above from 13/41 back down to 0/41.
pinned() {
  env -u VIM -u VIMRUNTIME -u LC_CTYPE -u LC_COLLATE -u LC_TIME \
      LC_ALL=C.UTF-8 LANG=C.UTF-8 LC_MESSAGES=C.UTF-8 LANGUAGE= TZ=UTC "$@"
}

# THE ENTRY POINT, in one place. Every run of the reference editor in this file
# goes through `$VIM_FLAGS`, and every report prints it — because the entry point
# decides the answer as much as the binary does. Measured on vim 9.2 (patches
# 1-1000) by replaying a driver of bare `&option` reads:
#
#   dropping `-N`        14 observables move (`&compatible`, `&fileformats`,
#                        `&backspace`, `&whichwrap`, `&history`, `&viminfo`,
#                        `&formatoptions`, `&modeline`, `&shortmess`, `&more`,
#                        `&ruler`, `&showcmd`, `&hlsearch`, `&cedit`)
#   dropping `-i NONE`   vim READS and WRITES ~/.viminfo, so one run mutates
#                        what the next one sees
#
# A record taken under one vector and checked under another is not a parity
# result, so the vector is stamped into `$STAMP` at record time and re-asserted
# on every run (see "entry-point assertions" below).
VIM_FLAGS="-es -u NONE -i NONE -N"

# run_vim FILE -> prints "status<NL>output"; the status is vim's, taken before
# the normaliser runs (a pipeline would otherwise report perl's).
run_vim() {
  local abs raw st
  abs=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
  # shellcheck disable=SC2086 — VIM_FLAGS is a deliberate word-split flag vector.
  raw=$(pinned "$VIM" $VIM_FLAGS -c "verbose source $abs" -c 'qa!' 2>&1); st=$?
  printf '%s\n' "$st"
  printf '%s' "$raw" | norm
}

run_viml() {
  local abs raw st
  abs=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
  raw=$(pinned "$VIML" "$abs" 2>&1); st=$?
  printf '%s\n' "$st"
  printf '%s' "$raw"
}

# ── entry-point assertions ──────────────────────────────────────────────────
#
# The pin is only worth anything if it TOOK, and each of these three has broken
# a run silently before:
#
#   encoding    a locale the C library does not have falls back to "C" without
#               saying so, which is the 9-record latin1 state.
#   compatible  `-N` is what separates the dialect this crate ports from the one
#               `-u NONE` leaves behind; without it 14 observables move and the
#               harness cannot see any of them.
#   ~/.viminfo  the most damaging failure, because it is invisible in the
#               output: a nocompatible vim without `-i NONE` reads the
#               developer's registers and histories and writes its own back, so
#               run N+1 answers differently from run N.
probe=$(pinned "$VIM" $VIM_FLAGS \
          -c 'verbose echo &encoding . "|" . &compatible' -c 'qa!' 2>&1 | tr -d '\r\n')
enc=${probe%%|*}
compat=${probe##*|}
if [ "$enc" != "utf-8" ]; then
  echo "the reference editor came up with encoding=$enc, not utf-8: this C library" >&2
  echo "does not have the C.UTF-8 locale, and every byte-level record would be" >&2
  echo "taken in latin1. Install/generate C.UTF-8 (or any UTF-8 locale and edit" >&2
  echo "REF_ENV) before running this harness." >&2
  exit 2
fi
if [ "$compat" != "0" ]; then
  echo "the reference editor came up with compatible=$compat under \`$VIM_FLAGS'." >&2
  echo "That is not the dialect this crate ports, and 14 observables differ" >&2
  echo "between the two states. The flag vector did not take." >&2
  exit 2
fi
VI_BEFORE=$(ls -l "$HOME/.viminfo" 2>/dev/null || echo none)
pinned "$VIM" $VIM_FLAGS -c 'qa!' >/dev/null 2>&1
if [ "$(ls -l "$HOME/.viminfo" 2>/dev/null || echo none)" != "$VI_BEFORE" ]; then
  echo "the reference editor WROTE ~/.viminfo under \`$VIM_FLAGS'. \`-i NONE' is not" >&2
  echo "in effect, so every record would carry the developer's registers and" >&2
  echo "histories and one probe would mutate what the next one sees." >&2
  exit 2
fi

# ── the self-report ─────────────────────────────────────────────────────────
#
# A differential harness that cannot NAME its own oracle is not one: a reader
# who cannot relaunch the exact editor a record came from cannot check it.
VIM_VERSION=$("$VIM" --version 2>/dev/null | head -1)
VIM_SHA=$( (shasum -a 256 "$VIM" 2>/dev/null || sha256sum "$VIM" 2>/dev/null) | cut -d' ' -f1)
STAMP="tests/parity_cases/ORACLE"
report_oracle() {
  printf "${C}oracle${N}  %s\n" "$VIM"
  printf "        %s\n" "$VIM_VERSION"
  printf "        sha256 %s\n" "$VIM_SHA"
  printf "        argv:  %s %s -c 'verbose source CASE' -c 'qa!'\n" "$VIM" "$VIM_FLAGS"
  printf "        env:   LC_ALL=C.UTF-8 LANG=C.UTF-8 LC_MESSAGES=C.UTF-8 LANGUAGE= TZ=UTC (VIM/VIMRUNTIME unset)\n"
  printf "        state: encoding=%s compatible=%s ~/.viminfo untouched\n" "$enc" "$compat"
}

# The records were taken through ONE entry point, and this says which. A record
# checked under a different one is not a parity result — so when the stamp and
# the live editor disagree, say so loudly rather than blaming the code for the
# difference.
check_stamp() {
  [ -f "$STAMP" ] || return 0
  local want
  want=$(grep '^flags:' "$STAMP" | cut -d' ' -f2-)
  if [ -n "$want" ] && [ "$want" != "$VIM_FLAGS" ]; then
    printf "${R}ENTRY POINT MOVED${N}  records were taken with \`%s', this run uses \`%s'\n" \
      "$want" "$VIM_FLAGS" >&2
    return 1
  fi
  local wantv
  wantv=$(grep '^version:' "$STAMP" | cut -d' ' -f2-)
  if [ -n "$wantv" ] && [ "$wantv" != "$VIM_VERSION" ]; then
    printf "${D}note: records were taken from %s; this run uses %s${N}\n" "$wantv" "$VIM_VERSION"
  fi
  return 0
}

# ── re-record ───────────────────────────────────────────────────────────────
#
# The expectation is vim's answer and nothing else. This is run after a fix, to
# accept behaviour that has already been read in a report — never to make a
# failing case quiet.
if [ -n "$RERECORD" ]; then
  [ -d "$RERECORD" ] || { echo "no corpus directory $RERECORD" >&2; exit 2; }
  n=0
  for case in "$RERECORD"/*.vim; do
    [ -e "$case" ] || continue
    run_vim "$case" >"${case%.vim}.expected"
    n=$((n + 1))
  done
  {
    printf 'binary: %s\n' "$VIM"
    printf 'sha256: %s\n' "$VIM_SHA"
    printf 'version: %s\n' "$VIM_VERSION"
    printf 'flags: %s\n' "$VIM_FLAGS"
    printf 'command: %s %s -c "verbose source CASE" -c "qa!"\n' "$VIM" "$VIM_FLAGS"
    printf 'env: LC_ALL=C.UTF-8 LANG=C.UTF-8 LC_MESSAGES=C.UTF-8 LANGUAGE= TZ=UTC (VIM/VIMRUNTIME unset)\n'
  } >"$STAMP"
  printf 'recorded %d case(s) into %s\n' "$n" "$RERECORD"
  report_oracle
  exit 0
fi

# ── one ad-hoc file ─────────────────────────────────────────────────────────
if [ -f "$TARGET" ]; then
  report_oracle
  run_vim  "$TARGET" >"$OUT/vim.txt"
  run_viml "$TARGET" >"$OUT/viml.txt"
  if cmp -s "$OUT/vim.txt" "$OUT/viml.txt"; then
    printf "${G}PARITY${N}  %s\n" "$TARGET"; exit 0
  fi
  printf "${R}DIVERGES${N}  %s  ${D}(line 1 of each block is the exit status)${N}\n" "$TARGET"
  diff -u --label vim "$OUT/vim.txt" --label viml "$OUT/viml.txt"
  exit 1
fi

# ── the corpus ──────────────────────────────────────────────────────────────
[ -d "$TARGET" ] || { echo "no such case file or directory: $TARGET" >&2; exit 2; }

SUB=$("$VIML" --version 2>/dev/null | tail -1)
say "checking $(find "$TARGET" -name '*.vim' | wc -l | tr -d ' ') case(s) with $SUB against:"
report_oracle
check_stamp || exit 2
echo

PASS=0 FAIL=0 STALE=0
: >"$OUT/diverge.txt"

for case in "$TARGET"/*.vim; do
  [ -e "$case" ] || continue
  name=$(basename "$case" .vim)
  exp="${case%.vim}.expected"
  run_viml "$case" >"$OUT/$name.viml"
  # The committed expectation is the oracle when it exists, so the corpus is a
  # CI gate that does not need vim installed to be meaningful. When vim *is*
  # installed the record is re-verified against it, and a stale record is
  # reported rather than trusted.
  if [ -f "$exp" ]; then
    cp "$exp" "$OUT/$name.vim.txt"
    run_vim "$case" >"$OUT/$name.live"
    if ! cmp -s "$exp" "$OUT/$name.live"; then
      STALE=$((STALE + 1))
      printf "${R}STALE RECORD${N} %s — this vim disagrees with the committed expectation\n" "$name"
      diff -u --label recorded "$exp" --label "live-vim" "$OUT/$name.live" | head -20
    fi
  else
    run_vim "$case" >"$OUT/$name.vim.txt"
  fi
  if cmp -s "$OUT/$name.vim.txt" "$OUT/$name.viml"; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
    {
      printf '=== %s ===\n' "$name"
      diff -u --label vim "$OUT/$name.vim.txt" --label viml "$OUT/$name.viml"
      printf '\n'
    } >>"$OUT/diverge.txt"
  fi
done

echo
printf '  %-22s %5d\n' "PASS (byte-identical)" "$PASS"
printf '  %-22s %5d\n' "DIVERGE"               "$FAIL"
printf '  %-22s %5d\n' "STALE RECORD"          "$STALE"
printf '  %-22s %s\n'  "artifacts"             "$OUT"

if [ "$FAIL" -eq 0 ] && [ "$STALE" -eq 0 ]; then
  echo
  printf "${G}PARITY: %d/%d cases produce byte-identical output and exit status.${N}\n" "$PASS" "$PASS"
  exit 0
fi
if [ "$QUIET" -eq 0 ] && [ "$FAIL" -gt 0 ]; then
  echo; say "divergences"; cat "$OUT/diverge.txt"
fi
BAD=$((FAIL + STALE))
[ "$BAD" -gt 250 ] && BAD=250
exit "$BAD"
