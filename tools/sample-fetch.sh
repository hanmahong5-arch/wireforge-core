#!/usr/bin/env bash
#
# sample-fetch.sh — D1 T0 mining for the Sample-First Wave.
#
# What this does:
#   1. For each known-OSS source listed in SOURCES below, clone the repo
#      shallow into candidates/<source-slug>/<source-slug>.git/, then look
#      for files whose names suggest they hold raw ISO 8583 hex test data.
#      Any matches are copied into candidates/<source-slug>/ alongside a
#      SOURCE.txt that records URL + commit + license + fetched_at.
#   2. For ad-hoc GitHub code search (channel #1 in the plan), the script
#      DOES NOT auto-execute `gh search code` because (a) it requires `gh`
#      auth and (b) the result set needs human judgement to filter
#      tutorial-grade blobs from real-shape ones. Instead it prints the
#      exact `gh` queries we would run; the operator pastes them.
#
# Honesty notes:
#   - The script does NOT promote anything into samples/iso8583/. That is
#     the sanitizer's job and requires per-file legal review (see plan §5
#     D3 _legal-review/ quarantine). All output stays under candidates/.
#   - candidates/ is git-ignored (see .gitignore at repo root). Nothing
#     this script writes will accidentally land in a commit.
#
# Usage:
#   tools/sample-fetch.sh          # run all sources
#   tools/sample-fetch.sh --dry    # print what would be done, fetch nothing
#   tools/sample-fetch.sh jpos     # run just the named source(s)
#
# Requires: bash, git. `gh` is OPTIONAL (only used for the code-search hint
# block, which is printed regardless).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CAND_ROOT="$ROOT/candidates"
DRY=0
FILTER=()

for arg in "$@"; do
    case "$arg" in
        --dry|--dry-run) DRY=1 ;;
        --help|-h)
            grep '^#' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) FILTER+=("$arg") ;;
    esac
done

want() {
    if [ ${#FILTER[@]} -eq 0 ]; then return 0; fi
    for f in "${FILTER[@]}"; do
        if [ "$f" = "$1" ]; then return 0; fi
    done
    return 1
}

run() {
    if [ "$DRY" -eq 1 ]; then
        printf 'DRY: %s\n' "$*"
    else
        eval "$@"
    fi
}

mkdir -p "$CAND_ROOT"

now_iso() {
    # GNU date is available under MSYS2; -u for UTC, ISO 8601 second precision.
    date -u +"%Y-%m-%dT%H:%M:%SZ"
}

write_source_txt() {
    local slug="$1" url="$2" commit="$3" license="$4"
    local out="$CAND_ROOT/$slug/SOURCE.txt"
    cat >"$out" <<EOF
slug = $slug
url = $url
commit = $commit
license = $license
fetched_at = $(now_iso)
EOF
}

# ---------------------------------------------------------------------------
# Source: jpos (Apache-2.0). Test resources under jpos/src/test/resources.
# Hit rate expectation: medium. Most jPOS fixtures are PEX (Java-side) or
# bitmap-table tests, not bare hex; treat ANY .hex/.bin discovered as worth
# review and let the sanitizer reject anything that doesn't parse.
# ---------------------------------------------------------------------------
fetch_jpos() {
    local slug=jpos
    want "$slug" || return 0
    local dir="$CAND_ROOT/$slug"
    local clone_dir="$dir/repo.git"
    local url="https://github.com/jpos/jPOS.git"
    local license="Apache-2.0"
    echo ">> fetching $slug ..."
    run "mkdir -p '$dir'"
    if [ ! -d "$clone_dir" ]; then
        run "git clone --depth=1 --quiet '$url' '$clone_dir'"
    fi
    local commit="unknown"
    if [ "$DRY" -eq 0 ] && [ -d "$clone_dir/.git" ]; then
        commit="$(git -C "$clone_dir" rev-parse HEAD)"
    fi
    [ "$DRY" -eq 0 ] && write_source_txt "$slug" "$url" "$commit" "$license"
    # Scan for hex-shaped fixture files.
    if [ "$DRY" -eq 0 ]; then
        find "$clone_dir" -type f \
            \( -iname '*.hex' -o -iname '*-iso8583*' -o -iname '*8583*.txt' \) \
            -print >"$dir/discovered.txt" || true
        echo "   discovered: $(wc -l <"$dir/discovered.txt") candidate file(s) (see discovered.txt)"
    fi
}

# ---------------------------------------------------------------------------
# Source: openiso8583-net (LGPL-2.1 — promotional ROI; check terms before
# redistributing transformed bytes). Lots of small .NET fixtures.
# ---------------------------------------------------------------------------
fetch_openiso8583() {
    local slug=openiso8583-net
    want "$slug" || return 0
    local dir="$CAND_ROOT/$slug"
    local clone_dir="$dir/repo.git"
    local url="https://github.com/openiso8583/openiso8583-net.git"
    local license="LGPL-2.1"
    echo ">> fetching $slug ..."
    run "mkdir -p '$dir'"
    if [ ! -d "$clone_dir" ]; then
        run "git clone --depth=1 --quiet '$url' '$clone_dir' || echo 'clone failed (network or repo gone)'"
    fi
    local commit="unknown"
    if [ "$DRY" -eq 0 ] && [ -d "$clone_dir/.git" ]; then
        commit="$(git -C "$clone_dir" rev-parse HEAD 2>/dev/null || echo unknown)"
    fi
    [ "$DRY" -eq 0 ] && write_source_txt "$slug" "$url" "$commit" "$license"
    if [ "$DRY" -eq 0 ] && [ -d "$clone_dir" ]; then
        find "$clone_dir" -type f \
            \( -iname '*.hex' -o -iname '*Sample*' -o -iname '*Test*.cs' \) \
            -print >"$dir/discovered.txt" || true
        echo "   discovered: $(wc -l <"$dir/discovered.txt") candidate file(s)"
    fi
}

# ---------------------------------------------------------------------------
# Source: golang iso8583 by moov-io (Apache-2.0). Generally has unit tests
# carrying bare hex. Highest a-priori hit rate of the T0 set.
# ---------------------------------------------------------------------------
fetch_moov_iso8583() {
    local slug=moov-iso8583
    want "$slug" || return 0
    local dir="$CAND_ROOT/$slug"
    local clone_dir="$dir/repo.git"
    local url="https://github.com/moov-io/iso8583.git"
    local license="Apache-2.0"
    echo ">> fetching $slug ..."
    run "mkdir -p '$dir'"
    if [ ! -d "$clone_dir" ]; then
        run "git clone --depth=1 --quiet '$url' '$clone_dir'"
    fi
    local commit="unknown"
    if [ "$DRY" -eq 0 ] && [ -d "$clone_dir/.git" ]; then
        commit="$(git -C "$clone_dir" rev-parse HEAD)"
    fi
    [ "$DRY" -eq 0 ] && write_source_txt "$slug" "$url" "$commit" "$license"
    if [ "$DRY" -eq 0 ]; then
        find "$clone_dir" -type f \
            \( -name '*_test.go' -o -name 'testdata*' -o -iname '*.hex' \) \
            -print >"$dir/discovered.txt" || true
        echo "   discovered: $(wc -l <"$dir/discovered.txt") candidate file(s)"
    fi
}

# ---------------------------------------------------------------------------
# `gh search code` hints — printed, not executed, because they need human
# filtering. Operator runs each query, eyeballs results, copies promising
# files into candidates/gh-search-<query-slug>/ by hand, then writes a
# SOURCE.txt before invoking the sanitizer.
# ---------------------------------------------------------------------------
print_gh_hints() {
    cat <<'EOF'

>> gh code-search queries (operator runs manually after `gh auth login`):

   gh search code --language=text 'iso8583 hex 0200'
   gh search code --extension=hex 'mti 0200 bitmap'
   gh search code 'PAN field 2 LLVAR'  --language=go
   gh search code 'jpos PackagerTest'   --language=java
   gh search code 'AcceptorTest'        --language=java
   gh search code 'iso8583 sample request response' --language=python

   For each hit:
     - record source URL + commit hash in candidates/gh-search-<slug>/SOURCE.txt
     - paste raw hex into candidates/gh-search-<slug>/raw.hex.raw
     - run: tools/sample-sanitize candidates/gh-search-<slug>/raw.hex.raw \
              --source gh-search-<slug> \
              --source-url <url> \
              --license <license-from-repo> \
              --out samples/iso8583/gh-search-<slug>-001.hex
EOF
}

main() {
    fetch_jpos
    fetch_openiso8583
    fetch_moov_iso8583
    print_gh_hints
    echo
    echo ">> done. Inspect candidates/ then run tools/sample-sanitize on each candidate."
}

main "$@"
