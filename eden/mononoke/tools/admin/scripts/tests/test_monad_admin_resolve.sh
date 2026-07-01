#!/bin/bash
# Tests for monad-admin-resolve. Runs the resolver against a mock `fbpkg`
# (tests/fbpkg_mock) and asserts version selection, staleness filtering,
# caching, and the fall-back exit contract.
#
# Run directly:  ./tests/test_monad_admin_resolve.sh
# Exits 0 if all cases pass, non-zero on the first failure.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESOLVER="$HERE/../monad-admin-resolve"
MOCK="$HERE/fbpkg_mock"

PASS=0
FAIL=0
ok()   { echo "ok   - $*"; PASS=$((PASS+1)); }
bad()  { echo "FAIL - $*"; FAIL=$((FAIL+1)); }

# Build a fresh sandbox: a MOCK_DIR of fixtures, a bin/ with `fbpkg` -> mock,
# and a clean cache. Echoes "MOCK_DIR CACHE BIN".
new_sandbox() {
    local root mockdir cache bindir
    root="$(mktemp -d)"
    mockdir="$root/mock"; cache="$root/cache"; bindir="$root/bin"
    mkdir -p "$mockdir" "$cache" "$bindir"
    ln -s "$MOCK" "$bindir/fbpkg"
    echo "$mockdir $cache $bindir"
}

# Standard three-consumer fixture set. lfs (2026-06-26) is the oldest.
write_consumers() {
    local mockdir="$1"
    cat > "$mockdir/info_mononoke.server.txt" <<'EOF'
    Version:                1366
    Build Repo Time:       2026-06-30 12:41:50
EOF
    cat > "$mockdir/info_mononoke.scs_server.txt" <<'EOF'
    Version:                1412
    Build Repo Time:       2026-06-30 00:41:41
EOF
    cat > "$mockdir/info_mononoke.lfs_server.txt" <<'EOF'
    Version:                1856
    Build Repo Time:       2026-06-26 04:09:11
EOF
}

# admin versions table: 2015..2017 straddle the 2026-06-26 target.
write_admin_versions() {
    local mockdir="$1"
    cat > "$mockdir/versions_mononoke.admin.txt" <<'EOF'
Gathering versions for mononoke.admin
Version ID             UUID                                Created                Expires    Archived    Tags
mononoke.admin:2017    9e4c559fa714c77a9ced89af07f99483    2026-06-27 01:44:56    NEVER      False
mononoke.admin:2016    dc4f5fc885a133cb2b0e48a76e08a182    2026-06-25 07:52:14    NEVER      False       prod-2026-06-26
mononoke.admin:2015    694312d4729cb933e32e177b421769b4    2026-06-25 00:46:42    NEVER      False
EOF
}

run_resolver() {
    # args: MOCK_DIR CACHE BIN [extra env assignments handled by caller]
    local mockdir="$1" cache="$2" bindir="$3"
    MOCK_DIR="$mockdir" \
    MONAD_CHECK_CACHE="$cache" \
    MONAD_CHECK_MAX_AGE_DAYS="${MAX_AGE:-100000}" \
    MOCK_FETCH_FAIL="${MOCK_FETCH_FAIL:-}" \
    PATH="$bindir:$PATH" \
        bash "$RESOLVER" 2>"$cache/stderr"
}

# --- case 1: picks newest admin at/before the oldest consumer, then fetches -
case_oldest_pick() {
    read -r m c b <<< "$(new_sandbox)"
    write_consumers "$m"; write_admin_versions "$m"
    local out rc
    out="$(run_resolver "$m" "$c" "$b")"; rc=$?
    # oldest consumer = lfs @ 2026-06-26 04:09:11.
    # newest admin Created <= that = 2016 (2026-06-25 07:52).
    if [[ $rc -eq 0 && "$out" == "$c/mononoke.admin/2016/admin" && -x "$out" ]]; then
        ok "oldest-pick resolves to admin 2016 and returns an executable"
    else
        bad "oldest-pick: rc=$rc out=$out"; cat "$c/stderr" >&2
    fi
    if [[ "$(cat "$m/fetch.log" 2>/dev/null)" == "mononoke.admin:2016" ]]; then
        ok "oldest-pick fetched exactly mononoke.admin:2016"
    else
        bad "oldest-pick fetch.log = $(cat "$m/fetch.log" 2>/dev/null)"
    fi
}

# --- case 2: a consumer older than MAX_AGE_DAYS is skipped, not chosen ------
case_stale_skip() {
    read -r m c b <<< "$(new_sandbox)"
    write_consumers "$m"; write_admin_versions "$m"
    # Add an ancient consumer; with the age filter on it must be ignored,
    # so the pick stays 2016 (driven by lfs), not the oldest-available admin.
    cat > "$m/info_mononoke.server.txt" <<'EOF'
    Build Repo Time:       2000-01-01 00:00:00
EOF
    local out rc
    MAX_AGE=60 out="$(run_resolver "$m" "$c" "$b")"; rc=$?
    if [[ $rc -eq 0 && "$out" == "$c/mononoke.admin/2016/admin" ]]; then
        ok "stale consumer (>60d) is skipped; pick driven by lfs = 2016"
    else
        bad "stale-skip: rc=$rc out=$out"; cat "$c/stderr" >&2
    fi
}

# --- case 3: cache hit avoids a second fetch --------------------------------
case_cache_hit() {
    read -r m c b <<< "$(new_sandbox)"
    write_consumers "$m"; write_admin_versions "$m"
    run_resolver "$m" "$c" "$b" >/dev/null   # warm the cache (fetch 2016)
    : > "$m/fetch.log"                        # reset the fetch record
    local out rc
    out="$(run_resolver "$m" "$c" "$b")"; rc=$?
    if [[ $rc -eq 0 && "$out" == "$c/mononoke.admin/2016/admin" && ! -s "$m/fetch.log" ]]; then
        ok "cache hit returns the path without re-fetching"
    else
        bad "cache-hit: rc=$rc out=$out fetch.log=$(cat "$c/fetch.log" 2>/dev/null)"
    fi
}

# --- case 4: no consumer resolvable -> non-zero (caller falls back) ---------
case_all_fail() {
    read -r m c b <<< "$(new_sandbox)"
    write_admin_versions "$m"   # admin exists, but no consumer fixtures
    local out rc
    out="$(run_resolver "$m" "$c" "$b")"; rc=$?
    if [[ $rc -ne 0 && -z "$out" ]] && grep -q "could not determine oldest deployed parser" "$c/stderr"; then
        ok "no resolvable consumer -> exit non-zero with warning"
    else
        bad "all-fail: rc=$rc out=$out"; cat "$c/stderr" >&2
    fi
}

# --- case 5: no admin at/before target -> oldest available + warn ----------
case_no_match_oldest_available() {
    read -r m c b <<< "$(new_sandbox)"
    write_consumers "$m"
    # All admin versions are NEWER than the oldest consumer (2026-06-26).
    cat > "$m/versions_mononoke.admin.txt" <<'EOF'
Version ID             UUID     Created                Expires  Archived  Tags
mononoke.admin:2100    uuid2    2026-06-29 01:00:00    NEVER    False
mononoke.admin:2099    uuid1    2026-06-28 01:00:00    NEVER    False
EOF
    local out rc
    out="$(run_resolver "$m" "$c" "$b")"; rc=$?
    if [[ $rc -eq 0 && "$out" == "$c/mononoke.admin/2099/admin" ]] \
        && grep -q "using oldest available" "$c/stderr"; then
        ok "no admin at/before target -> oldest available (2099) + warning"
    else
        bad "no-match: rc=$rc out=$out"; cat "$c/stderr" >&2
    fi
}

# --- case 6: fetch failure -> non-zero (caller falls back) ------------------
case_fetch_fail() {
    read -r m c b <<< "$(new_sandbox)"
    write_consumers "$m"; write_admin_versions "$m"
    local out rc
    MOCK_FETCH_FAIL=1 out="$(run_resolver "$m" "$c" "$b")"; rc=$?
    if [[ $rc -ne 0 && -z "$out" ]] && grep -q "fbpkg fetch mononoke.admin:2016 failed" "$c/stderr"; then
        ok "fetch failure -> exit non-zero with warning"
    else
        bad "fetch-fail: rc=$rc out=$out"; cat "$c/stderr" >&2
    fi
}

case_oldest_pick
case_stale_skip
case_cache_hit
case_all_fail
case_no_match_oldest_available
case_fetch_fail

echo "----"
echo "passed: $PASS  failed: $FAIL"
[[ $FAIL -eq 0 ]]
