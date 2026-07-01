#!/bin/bash
# Tests for monad-config-check's binary-selection logic: --local / env
# overrides / version-matched default / warn+fallback. The wrapper is copied
# next to a stub `monad-admin-resolve` so $SCRIPT_DIR resolution picks up the
# stub; `mononoke_admin` and the resolved binary are stubs that record that
# they ran and with which args.
#
# Run directly:  ./tests/test_monad_config_check.sh

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WRAPPER_SRC="$HERE/../monad-config-check"

PASS=0
FAIL=0
ok()  { echo "ok   - $*"; PASS=$((PASS+1)); }
bad() { echo "FAIL - $*"; FAIL=$((FAIL+1)); }

# Sandbox: a bin dir holding a copy of the wrapper + a stub resolver, a fake
# configerator root, a PATH mononoke_admin stub, and a marker/argv log.
# Echoes "ROOT BINDIR CONF MARKER ARGV".
new_sandbox() {
    local root bindir conf
    root="$(mktemp -d)"
    bindir="$root/bin"; conf="$root/conf"
    mkdir -p "$bindir" "$conf/materialized_configs/scm/mononoke/repos/tiers"
    echo '{}' > "$conf/materialized_configs/scm/mononoke/repos/tiers/scs.materialized_JSON"
    cp "$WRAPPER_SRC" "$bindir/monad-config-check"
    chmod +x "$bindir/monad-config-check"
    # PATH mononoke_admin stub: records identity + argv.
    cat > "$bindir/mononoke_admin" <<EOF
#!/bin/bash
echo PATH > "$root/marker"
printf '%s\n' "\$@" > "$root/argv"
exit 0
EOF
    chmod +x "$bindir/mononoke_admin"
    echo "$root $bindir $conf $root/marker $root/argv"
}

# Install a stub resolver next to the copied wrapper.
#   $1 = bindir, $2 = mode: "ok" (print a resolved-admin path) or "fail"
install_resolver() {
    local bindir="$1" mode="$2" root
    root="$(dirname "$bindir")"
    if [[ "$mode" == "ok" || "$mode" == "legacy" ]]; then
        # A resolved admin stub that records identity + argv. In "legacy"
        # mode it rejects the `config` subcommand (exit 2) to mimic an admin
        # older than the D110083435 `config check` subcommand.
        local legacy_guard=""
        # shellcheck disable=SC2016  # $1 is intentionally literal in the generated stub
        [[ "$mode" == "legacy" ]] && legacy_guard='if [[ "$1" == "config" ]]; then echo "error: unrecognized subcommand '"'"'config'"'"'" >&2; exit 2; fi'
        cat > "$bindir/resolved_admin" <<EOF
#!/bin/bash
$legacy_guard
echo RESOLVED > "$root/marker"
printf '%s\n' "\$@" > "$root/argv"
exit 0
EOF
        chmod +x "$bindir/resolved_admin"
        cat > "$bindir/monad-admin-resolve" <<EOF
#!/bin/bash
echo "$bindir/resolved_admin"
exit 0
EOF
    else
        cat > "$bindir/monad-admin-resolve" <<'EOF'
#!/bin/bash
echo "monad-admin-resolve: WARNING: simulated failure" >&2
exit 1
EOF
    fi
    chmod +x "$bindir/monad-admin-resolve"
}

run_wrapper() {
    # $1 = bindir, $2 = conf, rest = wrapper args; env passthrough via caller
    local bindir="$1" conf="$2"; shift 2
    CONFIGERATOR_ROOT="$conf" \
    MONONOKE_ADMIN_BIN="${MONONOKE_ADMIN_BIN:-}" \
    MONAD_CHECK_LOCAL="${MONAD_CHECK_LOCAL:-0}" \
    PATH="$bindir:$PATH" \
        bash "$bindir/monad-config-check" "$@" 2>"$conf/stderr"
}

# --- case 1: --local uses PATH admin, does NOT call the resolver -----------
case_local_flag() {
    read -r root bin conf marker argv <<< "$(new_sandbox)"
    # Resolver stub that would FAIL loudly if ever called.
    install_resolver "$bin" fail
    run_wrapper "$bin" "$conf" --local --repo aosp/manifest >/dev/null
    if [[ "$(cat "$marker" 2>/dev/null)" == "PATH" ]] \
        && ! grep -q "simulated failure" "$conf/stderr"; then
        ok "--local uses PATH mononoke_admin without invoking the resolver"
    else
        bad "local-flag: marker=$(cat "$marker" 2>/dev/null)"; cat "$conf/stderr" >&2
    fi
    # --local must be stripped from the args forwarded to `config check`.
    # Match whole lines (argv is one arg per line) so the wrapper's own
    # --local-configerator-path arg doesn't count as a substring hit.
    if grep -qx -- "--repo" "$argv" && ! grep -qx -- "--local" "$argv"; then
        ok "--local is stripped before reaching config check"
    else
        bad "local-flag argv: $(tr '\n' ' ' < "$argv")"
    fi
}

# --- case 2: default path uses the version-matched (resolved) admin --------
case_default_resolved() {
    read -r root bin conf marker argv <<< "$(new_sandbox)"
    install_resolver "$bin" ok
    run_wrapper "$bin" "$conf" --repo aosp/manifest >/dev/null
    if [[ "$(cat "$marker" 2>/dev/null)" == "RESOLVED" ]]; then
        ok "default path runs the version-matched admin from the resolver"
    else
        bad "default-resolved: marker=$(cat "$marker" 2>/dev/null)"; cat "$conf/stderr" >&2
    fi
}

# --- case 3: resolver failure -> fall back to PATH admin + warn ------------
case_fallback_on_failure() {
    read -r root bin conf marker argv <<< "$(new_sandbox)"
    install_resolver "$bin" fail
    run_wrapper "$bin" "$conf" --repo aosp/manifest >/dev/null
    if [[ "$(cat "$marker" 2>/dev/null)" == "PATH" ]] \
        && grep -q "could not resolve the version-matched admin" "$conf/stderr"; then
        ok "resolver failure falls back to PATH admin with a warning"
    else
        bad "fallback: marker=$(cat "$marker" 2>/dev/null)"; cat "$conf/stderr" >&2
    fi
}

# --- case 3b: resolved admin lacks `config check` -> fall back to PATH -----
case_legacy_admin_fallback() {
    read -r root bin conf marker argv <<< "$(new_sandbox)"
    install_resolver "$bin" legacy
    run_wrapper "$bin" "$conf" --repo aosp/manifest >/dev/null
    if [[ "$(cat "$marker" 2>/dev/null)" == "PATH" ]] \
        && grep -q "predates the 'config check' subcommand" "$conf/stderr"; then
        ok "version-matched admin without config check falls back to PATH"
    else
        bad "legacy-admin: marker=$(cat "$marker" 2>/dev/null)"; cat "$conf/stderr" >&2
    fi
}

# --- case 4: explicit MONONOKE_ADMIN_BIN wins, resolver not called ---------
case_env_override() {
    read -r root bin conf marker argv <<< "$(new_sandbox)"
    install_resolver "$bin" fail
    cat > "$bin/custom_admin" <<EOF
#!/bin/bash
echo CUSTOM > "$marker"
exit 0
EOF
    chmod +x "$bin/custom_admin"
    MONONOKE_ADMIN_BIN="$bin/custom_admin" \
        run_wrapper "$bin" "$conf" --repo aosp/manifest >/dev/null
    if [[ "$(cat "$marker" 2>/dev/null)" == "CUSTOM" ]] \
        && ! grep -q "simulated failure" "$conf/stderr"; then
        ok "explicit MONONOKE_ADMIN_BIN is used verbatim, resolver skipped"
    else
        bad "env-override: marker=$(cat "$marker" 2>/dev/null)"; cat "$conf/stderr" >&2
    fi
}

case_local_flag
case_default_resolved
case_fallback_on_failure
case_legacy_admin_fallback
case_env_override

echo "----"
echo "passed: $PASS  failed: $FAIL"
[[ $FAIL -eq 0 ]]
