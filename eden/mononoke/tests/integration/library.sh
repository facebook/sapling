#!/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Library routines and initial setup for Mononoke-related tests.

if [ -n "$FB_TEST_FIXTURES" ] && [ -f "$FB_TEST_FIXTURES/fb_library.sh" ]; then
  # shellcheck source=fbcode/eden/mononoke/tests/integration/facebook/fb_library.sh
  . "$FB_TEST_FIXTURES/fb_library.sh"
fi

PROXY_ID_TYPE="${FB_PROXY_ID_TYPE:-X509_SUBJECT_NAME}"
PROXY_ID_DATA="${FB_PROXY_ID_DATA:-CN=proxy,O=Mononoke,C=US,ST=CA}"
CLIENT0_ID_TYPE="${FB_CLIENT0_ID_TYPE:-X509_SUBJECT_NAME}"
CLIENT0_ID_DATA="${FB_CLIENT0_ID_DATA:-CN=client0,O=Mononoke,C=US,ST=CA}"
# shellcheck disable=SC2034
CLIENT1_ID_TYPE="${FB_CLIENT1_ID_TYPE:-X509_SUBJECT_NAME}"
# shellcheck disable=SC2034
CLIENT1_ID_DATA="${FB_CLIENT1_ID_DATA:-CN=client1,O=Mononoke,C=US,ST=CA}"
# shellcheck disable=SC2034
CLIENT2_ID_TYPE="${FB_CLIENT2_ID_TYPE:-X509_SUBJECT_NAME}"
# shellcheck disable=SC2034
CLIENT2_ID_DATA="${FB_CLIENT2_ID_DATA:-CN=client2,O=Mononoke,C=US,ST=CA}"
# shellcheck disable=SC2034
JSON_CLIENT_ID="${FB_JSON_CLIENT_ID:-[\"X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA\"]}"

if [[ -n "$DB_SHARD_NAME" ]]; then
  MONONOKE_DEFAULT_START_TIMEOUT=600
  MONONOKE_LFS_DEFAULT_START_TIMEOUT=60
  MONONOKE_SCS_DEFAULT_START_TIMEOUT=120
  MONONOKE_LAND_SERVICE_DEFAULT_START_TIMEOUT=120
else
  MONONOKE_DEFAULT_START_TIMEOUT=60
  MONONOKE_LFS_DEFAULT_START_TIMEOUT=60
  # First scsc call takes a while as scs server is doing derivation
  MONONOKE_SCS_DEFAULT_START_TIMEOUT=120
  MONONOKE_LAND_SERVICE_DEFAULT_START_TIMEOUT=120
  MONONOKE_DDS_DEFAULT_START_TIMEOUT=60
fi
VI_SERVICE_DEFAULT_START_TIMEOUT=60

function urlencode {
  "$URLENCODE" "$@"
}

REPOID=0
REPONAME=${REPONAME:-repo}

# Where we write host:port information after servers bind to :0
MONONOKE_SERVER_ADDR_FILE="$TESTTMP/mononoke_server_addr.txt"

export LOCAL_CONFIGERATOR_PATH="$TESTTMP/configerator"
mkdir -p "${LOCAL_CONFIGERATOR_PATH}"

export ACL_FILE="$TESTTMP/acls.json"

# The path for tunables. Do not write directly to this! Use merge_tunables instead.
export MONONOKE_TUNABLES_PATH="${LOCAL_CONFIGERATOR_PATH}/mononoke_tunables.json"

function get_configerator_relative_path {
  realpath --relative-to "${LOCAL_CONFIGERATOR_PATH}" "$1"
}

COMMON_ARGS=(
  --skip-caching
  --mysql-master-only
  --tunables-config "$(get_configerator_relative_path "${MONONOKE_TUNABLES_PATH}")"
  --local-configerator-path "${LOCAL_CONFIGERATOR_PATH}"
  --log-exclude-tag "futures_watchdog"
  --with-test-megarepo-configs-client=true
  --acl-file "${ACL_FILE}"
)

export TEST_CERTDIR
TEST_CERTDIR="${HGTEST_CERTDIR:-"$TEST_CERTS"}"
if [[ -z "$TEST_CERTDIR" ]]; then
  echo "TEST_CERTDIR is not set" 1>&2
  exit 1
fi

case "$(uname -s)" in
  # Workarounds for running tests on MacOS
  Darwin*)
    prefix="${HOMEBREW_PREFIX:-/usr/local}"

    # Use the brew installed versions of GNU utils
    PATH="$prefix/opt/gnu-sed/libexec/gnubin:\
$prefix/opt/grep/libexec/gnubin:\
$prefix/opt/coreutils/libexec/gnubin:\
$PATH"
  ;;
esac

function killandwait {
  # sends KILL to the given process and waits for it so that nothing is printed
  # to the terminal on MacOS
  { kill -9 "$1" && wait "$1"; } > /dev/null 2>&1
  # We don't care for wait exit code
  true
}

function get_free_socket {
  "$GET_FREE_SOCKET"
}

function mononoke_host {
  if [[ $LOCALIP == *":"* ]]; then
    # ipv6, surround in brackets
    echo -n "[$LOCALIP]"
  else
    echo -n "$LOCALIP"
  fi
}

function mononoke_address {
  if [[ $LOCALIP == *":"* ]]; then
    # ipv6, surround in brackets
    echo -n "[$LOCALIP]:$MONONOKE_SOCKET"
  else
    echo -n "$LOCALIP:$MONONOKE_SOCKET"
  fi
}

function scs_address {
  echo -n "$(mononoke_host):$SCS_PORT"
}

function land_service_address {
  echo -n "$(mononoke_host):$LAND_SERVICE_PORT"
}

# return random value from [1, max_value]
function random_int() {
  max_value=$1

  VAL=$RANDOM
  (( VAL %= max_value ))
  (( VAL += 1 ))

  echo $VAL
}

function sslcurlas {
  local name="$1"
  shift
  curl --noproxy localhost --cert "$TEST_CERTDIR/$name.crt" --cacert "$TEST_CERTDIR/root-ca.crt" --key "$TEST_CERTDIR/$name.key" "$@"
}

function sslcurl {
  sslcurlas proxy "$@"
}

function mononoke {
  SCRIBE_LOGS_DIR="$TESTTMP/scribe_logs"
  if [[ ! -d "$SCRIBE_LOGS_DIR" ]]; then
    mkdir "$SCRIBE_LOGS_DIR"
  fi

  setup_configerator_configs

  local BIND_ADDR
  if [[ $LOCALIP == *":"* ]]; then
    # ipv6, surround in brackets
    BIND_ADDR="[$LOCALIP]:0"
  else
    BIND_ADDR="$LOCALIP:0"
  fi

  # Stop any confusion from previous runs
  rm -f "$MONONOKE_SERVER_ADDR_FILE"

  # Ignore specific Python warnings to make tests predictable.
  PYTHONWARNINGS="ignore:::requests,ignore::SyntaxWarning" \
  GLOG_minloglevel=5 \
    "$MONONOKE_SERVER" "$@" \
    --scribe-logging-directory "$TESTTMP/scribe_logs" \
    --ca-pem "$TEST_CERTDIR/root-ca.crt" \
    --private-key "$TEST_CERTDIR/localhost.key" \
    --cert "$TEST_CERTDIR/localhost.crt" \
    --ssl-ticket-seeds "$TEST_CERTDIR/server.pem.seeds" \
    --land-service-client-cert="$TEST_CERTDIR/proxy.crt" \
    --land-service-client-private-key="$TEST_CERTDIR/proxy.key" \
    --debug \
    --listening-host-port "$BIND_ADDR" \
    --bound-address-file "$MONONOKE_SERVER_ADDR_FILE" \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --no-default-scuba-dataset \
    "${COMMON_ARGS[@]}" >> "$TESTTMP/mononoke.out" 2>&1 &
  export MONONOKE_PID=$!
  echo "$MONONOKE_PID" >> "$DAEMON_PIDS"
}

function mononoke_hg_sync {
  HG_REPO="$1"
  shift
  START_ID="$1"
  shift

  GLOG_minloglevel=5 "$MONONOKE_HG_SYNC" \
    "${COMMON_ARGS[@]}" \
    --retry-num 1 \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --verify-server-bookmark-on-failure \
     ssh://user@dummy/"$HG_REPO" "$@" sync-once --start-id "$START_ID"
}

function mononoke_backup_sync {
  HG_REPO="$1"
  SYNC_MODE="$2"
  START_ID="$3"
  shift
  shift
  shift

  GLOG_minloglevel=5 "$MONONOKE_HG_SYNC" \
    "${COMMON_ARGS[@]}" \
    --retry-num 1 \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --verify-server-bookmark-on-failure \
    --darkstorm-backup-repo-id "$BACKUP_REPO_ID" \
    "mononoke://$(mononoke_address)/$HG_REPO" "$@" "$SYNC_MODE" --start-id "$START_ID"
}

function mononoke_backup_sync_loop_forever {
  HG_REPO="$1"
  START_ID="$2"
  shift
  shift

  GLOG_minloglevel=5 "$MONONOKE_HG_SYNC" \
    "${COMMON_ARGS[@]}" \
    --retry-num 1 \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --verify-server-bookmark-on-failure \
    --darkstorm-backup-repo-id "$BACKUP_REPO_ID" \
    "mononoke://$(mononoke_address)/$HG_REPO" "$@" \
    sync-loop \
    --start-id "$START_ID" \
    --loop-forever  >> "$TESTTMP/backup_sync.out" 2>&1 &
  export BACKUP_SYNC_PID=$!
  echo "$BACKUP_SYNC_PID" >> "$DAEMON_PIDS"
}


function megarepo_tool {
  GLOG_minloglevel=5 "$MEGAREPO_TOOL" \
    "${COMMON_ARGS[@]}" \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
    "$@"
}


function megarepo_tool_multirepo {
  GLOG_minloglevel=5 "$MEGAREPO_TOOL" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
    "$@"
}

function mononoke_walker {
  GLOG_minloglevel=5 "$MONONOKE_WALKER" \
    "${COMMON_ARGS[@]}" \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
    "$@"
}

function mononoke_blobstore_healer {
  GLOG_minloglevel=5 "$MONONOKE_BLOBSTORE_HEALER" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
    "$@" 2>&1 | grep -v "Could not connect to a replica"
}

function mononoke_sqlblob_gc {
  GLOG_minloglevel=5 "$MONONOKE_SQLBLOB_GC" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
    "$@" 2>&1 | grep -v "Could not connect to a replica"
}

function mononoke_x_repo_sync() {
  source_repo_id=$1
  target_repo_id=$2
  shift
  shift
  GLOG_minloglevel=5 "$MONONOKE_X_REPO_SYNC" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --source-repo-id "$source_repo_id" \
    --target-repo-id "$target_repo_id" \
    "$@"
}

function mononoke_rechunker {
    GLOG_minloglevel=5 "$MONONOKE_RECHUNKER" \
    "${COMMON_ARGS[@]}" \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
    "$@"
}

function mononoke_hg_sync_with_retry {
  GLOG_minloglevel=5 "$MONONOKE_HG_SYNC" \
    "${COMMON_ARGS[@]}" \
    --base-retry-delay-ms 1 \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
    --verify-server-bookmark-on-failure \
     ssh://user@dummy/"$1" sync-once --start-id "$2"
}

function mononoke_hg_sync_with_failure_handler {
  GLOG_minloglevel=5 "$MONONOKE_HG_SYNC" \
    "${COMMON_ARGS[@]}" \
    --retry-num 1 \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
    --verify-server-bookmark-on-failure \
    --lock-on-failure \
     ssh://user@dummy/"$1" sync-once --start-id "$2"
}

function create_books_sqlite3_db {
  cat >> "$TESTTMP"/bookmarks.sql <<SQL
  CREATE TABLE IF NOT EXISTS bookmarks_update_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  repo_id INT UNSIGNED NOT NULL,
  name VARCHAR(512) NOT NULL,
  from_changeset_id VARBINARY(32),
  to_changeset_id VARBINARY(32),
  reason VARCHAR(32) NOT NULL, -- enum is used in mysql
  timestamp BIGINT NOT NULL
);
SQL

  sqlite3 "$TESTTMP/monsql/sqlite_dbs" < "$TESTTMP"/bookmarks.sql
}

function mononoke_hg_sync_loop {
  local repo="$1"
  local start_id="$2"
  shift
  shift

  GLOG_minloglevel=5 "$MONONOKE_HG_SYNC" \
    "${COMMON_ARGS[@]}" \
    --retry-num 1 \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
    ssh://user@dummy/"$repo" sync-loop --start-id "$start_id" "$@"
}

function mononoke_hg_sync_loop_regenerate {
  local repo="$1"
  local start_id="$2"
  shift
  shift

  GLOG_minloglevel=5 "$MONONOKE_HG_SYNC" \
    "${COMMON_ARGS[@]}" \
    --retry-num 1 \
    --repo-id 0 \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
    ssh://user@dummy/"$repo" sync-loop --start-id "$start_id" "$@"
}

function mononoke_admin {
  GLOG_minloglevel=5 "$MONONOKE_ADMIN" \
    "${COMMON_ARGS[@]}" \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config "$@"
}

function mononoke_newadmin {
  GLOG_minloglevel=5 "$MONONOKE_NEWADMIN" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "$TESTTMP"/mononoke-config "$@"
}

function mononoke_import {
  GLOG_minloglevel=5 "$MONONOKE_IMPORT" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "$TESTTMP"/mononoke-config "$@"
}

function mononoke_testtool {
  GLOG_minloglevel=5 "$MONONOKE_TESTTOOL" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "$TESTTMP"/mononoke-config "$@"
}

function mononoke_admin_source_target {
  local source_repo_id=$1
  shift
  local target_repo_id=$1
  shift
  GLOG_minloglevel=5 "$MONONOKE_ADMIN" \
    "${COMMON_ARGS[@]}" \
    --source-repo-id "$source_repo_id" \
    --target-repo-id "$target_repo_id" \
    --mononoke-config-path "$TESTTMP"/mononoke-config "$@"
}

function mononoke_hyper_repo_builder {
  local source_repo_book=$1
  shift
  local target_repo_book=$1
  shift
  GLOG_minloglevel=5 "$MONONOKE_HYPER_REPO_BUILDER" \
    "${COMMON_ARGS[@]}" \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
    --source-repo-bookmark-name "$source_repo_book" \
    --hyper-repo-bookmark-name "$target_repo_book" \
    "$@"
}

# Remove the glog prefix
function strip_glog {
  # based on https://our.internmc.facebook.com/intern/wiki/LogKnock/Log_formats/#regex-for-glog
  sed -E -e 's%^[VDIWECF][[:digit:]]{4} [[:digit:]]{2}:?[[:digit:]]{2}:?[[:digit:]]{2}(\.[[:digit:]]+)?\s+(([0-9a-f]+)\s+)?(\[([^]]+)\]\s+)?(\(([^\)]+)\)\s+)?(([a-zA-Z0-9_./-]+):([[:digit:]]+))\]\s+%%'
}

function wait_for_json_record_count {
  # We ask jq to count records for us, so that we're a little more robust ot
  # newlines and such.
  local file count
  file="$1"
  count="$2"

  for _ in $(seq 1 50); do
    if [[ "$(jq 'true' < "$file" | wc -l)" -eq "$count" ]] ; then
      return 0
    fi

    sleep 0.1
  done

  echo "File $file did not contain $count records, it had $(jq 'true' < "$file" | wc -l)" >&2
  jq -S . < "$file" >&2
  return 1
}

function wait_for_server {
  local service_description port_env_var log_file timeout_secs bound_addr_file
  service_description="$1"; shift
  port_env_var="$1"; shift
  log_file="$1"; shift
  timeout_secs="$1"; shift
  bound_addr_file="$1"; shift

  local start
  start=$(date +%s)

  local found_port
  while [[ $(($(date +%s) - start)) -lt "$timeout_secs" ]]; do
    if [[ -z "$found_port" && -r "$bound_addr_file" ]]; then
      found_port=$(sed 's/^.*:\([^:]*\)$/\1/' "$bound_addr_file")
      eval "$port_env_var"="$found_port"
    fi
    if [[ -n "$found_port" ]] && "$@" >/dev/null 2>&1 ; then
      return 0
    fi
    sleep 0.1
  done

  echo "$service_description did not start in $timeout_secs seconds, took $(($(date +%s) - start))" >&2
  if [[ -n "$found_port" ]]; then
    echo "Running check: $* >/dev/null"
    "$@" >/dev/null
    echo "exited with $?" 1>&2
  else
    echo "Port was never written to $bound_addr_file" 1>&2
  fi
  echo "" 1>&2
  echo "Log of $service_description" 1>&2
  cat "$log_file" 1>&2
  exit 1
}

function mononoke_health {
  sslcurl -q "https://localhost:$MONONOKE_SOCKET/health_check"
}

# Wait until a Mononoke server is available for this repo.
function wait_for_mononoke {
  export MONONOKE_SOCKET
  wait_for_server "Mononoke" MONONOKE_SOCKET "$TESTTMP/mononoke.out" \
    "${MONONOKE_START_TIMEOUT:-"$MONONOKE_DEFAULT_START_TIMEOUT"}" "$MONONOKE_SERVER_ADDR_FILE" \
    mononoke_health
}

function flush_mononoke_bookmarks {
  sslcurl -X POST -fsS "https://localhost:$MONONOKE_SOCKET/control/drop_bookmarks_cache"
}

function force_update_configerator {
  sslcurl -X POST -fsS "https://localhost:$MONONOKE_SOCKET/control/force_update_configerator"
}

function start_and_wait_for_mononoke_server {
    mononoke "$@"
    wait_for_mononoke
}

# Wait until cache warmup finishes
function wait_for_mononoke_cache_warmup {
  local attempts=150
  for _ in $(seq 1 $attempts); do
    grep -q "finished initial warmup" "$TESTTMP/mononoke.out" && break
    sleep 0.1
  done

  if ! grep -q "finished initial warmup" "$TESTTMP/mononoke.out"; then
    echo "Mononoke warmup did not finished" >&2
    cat "$TESTTMP/mononoke.out"
    exit 1
  fi
}

function setup_common_hg_configs {
  cat >> "$HGRCPATH" <<EOF
[ui]
ssh="$DUMMYSSH"
[devel]
segmented-changelog-rev-compat=True
[extensions]
remotefilelog=
[remotefilelog]
cachepath=$TESTTMP/cachepath
[extensions]
commitextras=
[hint]
ack=*
[experimental]
changegroup3=True
[mutation]
record=False
[web]
cacerts=$TEST_CERTDIR/root-ca.crt
[auth]
mononoke.cert=$TEST_CERTDIR/${OVERRIDE_CLIENT_CERT:-client0}.crt
mononoke.key=$TEST_CERTDIR/${OVERRIDE_CLIENT_CERT:-client0}.key
mononoke.prefix=mononoke://*
mononoke.cn=localhost
edenapi.cert=$TEST_CERTDIR/${OVERRIDE_CLIENT_CERT:-client0}.crt
edenapi.key=$TEST_CERTDIR/${OVERRIDE_CLIENT_CERT:-client0}.key
edenapi.prefix=localhost
edenapi.cacerts=$TEST_CERTDIR/root-ca.crt
[workingcopy]
use-rust=False
ruststatus=False
[status]
use-rust=False
EOF
}

function setup_common_config {
    setup_mononoke_config "$@"
    setup_common_hg_configs
    setup_configerator_configs
}

function get_bonsai_svnrev_mapping {
  sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select hex(bcs_id), svnrev from bonsai_svnrev_mapping order by id";
}

function get_bonsai_git_mapping {
  sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select hex(bcs_id), hex(git_sha1) from bonsai_git_mapping order by id";
}

function get_bonsai_hg_mapping {
  sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select hex(bcs_id), hex(hg_cs_id) from bonsai_hg_mapping order by repo_id, bcs_id";
}

function get_bonsai_globalrev_mapping {
  sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select hex(bcs_id), globalrev from bonsai_globalrev_mapping order by globalrev";
}

function setup_mononoke_config {
  cd "$TESTTMP" || exit

  mkdir -p mononoke-config
  REPOTYPE="blob_sqlite"
  if [[ $# -gt 0 ]]; then
    REPOTYPE="$1"
    shift
  fi
  local blobstorename=blobstore
  if [[ $# -gt 0 ]]; then
    blobstorename="$1"
    shift
  fi

  cd mononoke-config || exit 1
  mkdir -p common
  touch common/common.toml
  touch common/commitsyncmap.toml

  # We have some tests that call this twice...
  truncate -s 0 common/common.toml

  if [[ -n "$SCUBA_CENSORED_LOGGING_PATH" ]]; then
  cat > common/common.toml <<CONFIG
scuba_local_path_censored="$SCUBA_CENSORED_LOGGING_PATH"
CONFIG
  fi

  if [[ -z "$DISABLE_HTTP_CONTROL_API" ]]; then
  cat >> common/common.toml <<CONFIG
enable_http_control_api=true
CONFIG
  fi

  cat >> common/common.toml <<CONFIG
[internal_identity]
identity_type = "SERVICE_IDENTITY"
identity_data = "proxy"

[redaction_config]
blobstore = "$blobstorename"
darkstorm_blobstore = "$blobstorename"
redaction_sets_location = "scm/mononoke/redaction/redaction_sets"

[[trusted_parties_allowlist]]
identity_type = "$PROXY_ID_TYPE"
identity_data = "$PROXY_ID_DATA"
CONFIG

  cat >> common/common.toml <<CONFIG
${ADDITIONAL_MONONOKE_COMMON_CONFIG}
CONFIG

  echo "# Start new config" > common/storage.toml
  setup_mononoke_storage_config "$REPOTYPE" "$blobstorename"

  setup_mononoke_repo_config "$REPONAME" "$blobstorename"

  setup_acls
}

function setup_acls() {
  if [[ ! -f "$ACL_FILE" ]]; then
    cat > "$ACL_FILE" <<ACLS
{
  "repos": {
    "default": {
      "actions": {
        "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"],
        "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA"]
      }
    }
  }
}
ACLS
  fi
}

function db_config() {
  local blobstorename="$1"
  if [[ -n "$DB_SHARD_NAME" ]]; then
    echo "[$blobstorename.metadata.remote]"
    echo "primary = { db_address = \"$DB_SHARD_NAME\" }"
    echo "filenodes = { unsharded = { db_address = \"$DB_SHARD_NAME\" } }"
    echo "mutation = { db_address = \"$DB_SHARD_NAME\" }"
  else
    echo "[$blobstorename.metadata.local]"
    echo "local_db_path = \"$TESTTMP/monsql\""
  fi
}

function ephemeral_db_config() {
  local blobstorename="$1"
  if [[ -n "$DB_SHARD_NAME" ]]; then
    echo "[$blobstorename.metadata.remote]"
    echo "db_address = \"$DB_SHARD_NAME\""
  else
    echo "[$blobstorename.metadata.local]"
    echo "local_db_path = \"$TESTTMP/monsql\""
  fi
}

function blobstore_db_config() {
  if [[ -n "$DB_SHARD_NAME" ]]; then
    echo "queue_db = { remote = { db_address = \"$DB_SHARD_NAME\" } }"
  else
    local blobstore_db_path="$TESTTMP/blobstore_sync_queue"
    mkdir -p "$blobstore_db_path"
    echo "queue_db = { local = { local_db_path = \"$blobstore_db_path\" } }"
  fi
}

function setup_mononoke_storage_config {
  local underlyingstorage="$1"
  local blobstorename="$2"
  local blobstorepath="$TESTTMP/$blobstorename"
  local bubble_deletion_mode=0 # Bubble deletion is disabled by default
  if [[ -n ${BUBBLE_DELETION_MODE:-} ]]; then
    bubble_deletion_mode=${BUBBLE_DELETION_MODE}
  fi
  local bubble_lifespan_secs=1000
  if [[ -n ${BUBBLE_LIFESPAN_SECS:-} ]]; then
    bubble_lifespan_secs=${BUBBLE_LIFESPAN_SECS}
  fi
  local bubble_expiration_secs=1000
  if [[ -n ${BUBBLE_EXPIRATION_SECS:-} ]]; then
    bubble_expiration_secs=${BUBBLE_EXPIRATION_SECS}
  fi

  if [[ -n "${MULTIPLEXED:-}" ]]; then
    local quorum
    local btype
    local scuba
    if [[ "$WAL" != "" ]]; then
      quorum="write_quorum"
      btype="multiplexed_wal"
      scuba="multiplex_scuba_table = \"file://$TESTTMP/blobstore_trace_scuba.json\""
    else
      quorum="minimum_successful_writes"
      btype="multiplexed"
      scuba=""
    fi
    cat >> common/storage.toml <<CONFIG
$(db_config "$blobstorename")

[$blobstorename.blobstore.${btype}]
multiplex_id = 1
$(blobstore_db_config)
${quorum} = ${MULTIPLEXED}
${scuba}
components = [
CONFIG

    local i
    for ((i=0; i<=MULTIPLEXED; i++)); do
      mkdir -p "$blobstorepath/$i/blobs"
      if [[ -n "${PACK_BLOB:-}" && $i -le "$PACK_BLOB" ]]; then
        echo "  { blobstore_id = $i, blobstore = { pack = { blobstore = { $underlyingstorage = { path = \"$blobstorepath/$i\" } } } } }," >> common/storage.toml
      else
        echo "  { blobstore_id = $i, blobstore = { $underlyingstorage = { path = \"$blobstorepath/$i\" } } }," >> common/storage.toml
      fi
    done
    echo ']' >> common/storage.toml
  else
    mkdir -p "$blobstorepath/blobs"
    # Using FileBlob instead of SqlBlob as the backing blobstore for ephemeral
    # store since SqlBlob current doesn't support enumeration.
    cat >> common/storage.toml <<CONFIG
$(db_config "$blobstorename")

[$blobstorename.ephemeral_blobstore]
initial_bubble_lifespan_secs = $bubble_lifespan_secs
bubble_expiration_grace_secs = $bubble_expiration_secs
bubble_deletion_mode = $bubble_deletion_mode
blobstore = { blob_files = { path = "$blobstorepath" } }

$(ephemeral_db_config "$blobstorename.ephemeral_blobstore")


[$blobstorename.blobstore]
CONFIG
    if [[ -n "${PACK_BLOB:-}" ]]; then
      echo "  pack = { blobstore = { $underlyingstorage = { path = \"$blobstorepath\" } } }" >> common/storage.toml
    else
      echo "  $underlyingstorage = { path = \"$blobstorepath\" }" >> common/storage.toml
    fi
  fi
}

function setup_commitsyncmap {
  cp "$TEST_FIXTURES/commitsync/current.toml" "$TESTTMP/mononoke-config/common/commitsyncmap.toml"
}

function setup_configerator_configs {
  export RATE_LIMIT_CONF
  RATE_LIMIT_CONF="${LOCAL_CONFIGERATOR_PATH}/scm/mononoke/ratelimiting"
  mkdir -p "$RATE_LIMIT_CONF"

  if [[ ! -f "$RATE_LIMIT_CONF/ratelimits" ]]; then
    cat >> "$RATE_LIMIT_CONF/ratelimits" <<EOF
{
  "rate_limits": [],
  "load_shed_limits": [],
  "datacenter_prefix_capacity": {},
  "commits_per_author": {
    "status": 0,
    "limit": 300,
    "window": 1800
  },
  "total_file_changes": {
    "status": 0,
    "limit": 80000,
    "window": 5
  }
}
EOF
  fi

  export PUSHREDIRECT_CONF
  PUSHREDIRECT_CONF="${LOCAL_CONFIGERATOR_PATH}/scm/mononoke/pushredirect"
  mkdir -p "$PUSHREDIRECT_CONF"

  if [[ ! -f "$PUSHREDIRECT_CONF/enable" ]]; then
    cat >> "$PUSHREDIRECT_CONF/enable" <<EOF
{
  "per_repo": {}
}
EOF
  fi

  COMMIT_SYNC_CONF="${LOCAL_CONFIGERATOR_PATH}/scm/mononoke/repos/commitsyncmaps"
  mkdir -p "$COMMIT_SYNC_CONF"
  export COMMIT_SYNC_CONF
  if [[ -n $SKIP_CROSS_REPO_CONFIG ]]; then
    cat > "$COMMIT_SYNC_CONF/all" <<EOF
{
}
EOF
    cat > "$COMMIT_SYNC_CONF/current" <<EOF
{
}
EOF
  else
    if [[ ! -f "$COMMIT_SYNC_CONF/all" ]]; then
      cp "$TEST_FIXTURES/commitsync/all.json" "$COMMIT_SYNC_CONF/all"
    fi
    if [[ ! -f "$COMMIT_SYNC_CONF/current" ]]; then
      cp "$TEST_FIXTURES/commitsync/current.json" "$COMMIT_SYNC_CONF/current"
    fi
  fi

  export XDB_GC_CONF
  XDB_GC_CONF="${LOCAL_CONFIGERATOR_PATH}/scm/mononoke/xdb_gc"
  mkdir -p "$XDB_GC_CONF"
  if [[ ! -f "$XDB_GC_CONF/default" ]]; then
    cat >> "$XDB_GC_CONF/default" <<EOF
{
  "put_generation": 2,
  "mark_generation": 1,
  "delete_generation": 0
}
EOF
  fi

  export OBSERVABILITY_CONF
  OBSERVABILITY_CONF="${LOCAL_CONFIGERATOR_PATH}/scm/mononoke/observability"
  mkdir -p "$OBSERVABILITY_CONF"
  if [[ ! -f "$OBSERVABILITY_CONF/observability_config" ]]; then
  cat >> "$OBSERVABILITY_CONF/observability_config" <<EOF
{
  "slog_config": {
    "level": 4
  },
  "scuba_config": {
    "level": 1,
    "verbose_sessions": [],
    "verbose_unixnames": [],
    "verbose_source_hostnames": []
  }
}
EOF
  fi

  export REDACTION_CONF
  REDACTION_CONF="${LOCAL_CONFIGERATOR_PATH}/scm/mononoke/redaction"
  mkdir -p "$REDACTION_CONF"

  if [[ ! -f "$REDACTION_CONF/redaction_sets" ]]; then
    cat >> "$REDACTION_CONF/redaction_sets" <<EOF
{
  "all_redactions": []
}
EOF
  fi


  export REPLICATION_LAG_CONF
  REPLICATION_LAG_CONF="${LOCAL_CONFIGERATOR_PATH}/scm/mononoke/mysql/replication_lag/config"
  mkdir -p "$REPLICATION_LAG_CONF"
  for CONF in "healer" "derived_data_backfiller" "derived_data_tailer"; do
    if [[ ! -f "$REPLICATION_LAG_CONF/$CONF" ]]; then
      cat >> "$REPLICATION_LAG_CONF/$CONF" <<EOF
 {
 }
EOF
    fi
  done
}

function setup_mononoke_repo_config {
  cd "$TESTTMP/mononoke-config" || exit
  local reponame="$1"
  local reponame_urlencoded
  reponame_urlencoded="$(urlencode encode "$reponame")"
  local storageconfig="$2"
  mkdir -p "repos/$reponame_urlencoded"
  mkdir -p "repo_definitions/$reponame_urlencoded"
  mkdir -p "$TESTTMP/monsql"
  mkdir -p "$TESTTMP/$reponame_urlencoded"
  mkdir -p "$TESTTMP/traffic-replay-blobstore"
  cat > "repos/$reponame_urlencoded/server.toml" <<CONFIG
hash_validation_percentage=100
CONFIG

  cat > "repo_definitions/$reponame_urlencoded/server.toml" <<CONFIG
repo_id=$REPOID
repo_name="$reponame"
repo_config="$reponame"
enabled=${ENABLED:-true}
hipster_acl="${ACL_NAME:-default}"
CONFIG


if [[ -n "${READ_ONLY_REPO:-}" ]]; then
  cat >> "repo_definitions/$reponame_urlencoded/server.toml" <<CONFIG
readonly=true
CONFIG
fi

if [[ -n "${SCUBA_LOGGING_PATH:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
scuba_local_path="$SCUBA_LOGGING_PATH"
CONFIG
fi

if [[ -n "${ENFORCE_LFS_ACL_CHECK:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
enforce_lfs_acl_check=true
CONFIG
fi

if [[ -n "${REPO_CLIENT_USE_WARM_BOOKMARKS_CACHE:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
repo_client_use_warm_bookmarks_cache=true
CONFIG
fi

if [[ -n "${SKIPLIST_INDEX_BLOBSTORE_KEY:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
skiplist_index_blobstore_key="$SKIPLIST_INDEX_BLOBSTORE_KEY"
CONFIG
fi

# Normally point to common storageconfig, but if none passed, create per-repo
if [[ -z "$storageconfig" ]]; then
  storageconfig="blobstore_$reponame_urlencoded"
  setup_mononoke_storage_config "$REPOTYPE" "$storageconfig"
fi
cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
storage_config = "$storageconfig"

CONFIG

if [[ -n "${FILESTORE:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[filestore]
chunk_size = ${FILESTORE_CHUNK_SIZE:-10}
concurrency = 24
CONFIG
fi

if [[ -n "${REDACTION_DISABLED:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
redaction=false
CONFIG
fi

if [[ -n "${LIST_KEYS_PATTERNS_MAX:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
list_keys_patterns_max=$LIST_KEYS_PATTERNS_MAX
CONFIG
fi

if [[ -n "${ONLY_FAST_FORWARD_BOOKMARK:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[[bookmarks]]
name="$ONLY_FAST_FORWARD_BOOKMARK"
only_fast_forward=true
CONFIG
fi

if [[ -n "${ONLY_FAST_FORWARD_BOOKMARK_REGEX:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[[bookmarks]]
regex="$ONLY_FAST_FORWARD_BOOKMARK_REGEX"
only_fast_forward=true
CONFIG
fi

  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[pushrebase]
forbid_p2_root_rebases=false
CONFIG

if [[ -n "${COMMIT_SCRIBE_CATEGORY:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
commit_scribe_category = "$COMMIT_SCRIBE_CATEGORY"
CONFIG
fi

if [[ -n "${ALLOW_CASEFOLDING:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
casefolding_check=false
CONFIG
fi

if [[ -n "${BLOCK_MERGES:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
block_merges=true
CONFIG
fi

if [[ -n "${PUSHREBASE_REWRITE_DATES:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
rewritedates=true
CONFIG
else
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
rewritedates=false
CONFIG
fi

if [[ -n "${EMIT_OBSMARKERS:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
emit_obsmarkers=true
CONFIG
fi

if [[ -n "${GLOBALREVS_PUBLISHING_BOOKMARK:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
globalrevs_publishing_bookmark = "${GLOBALREVS_PUBLISHING_BOOKMARK}"
CONFIG
fi

if [[ -n "${POPULATE_GIT_MAPPING:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
populate_git_mapping=true
CONFIG
fi

if [[ -n "${ALLOW_CHANGE_XREPO_MAPPING_EXTRA:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
allow_change_xrepo_mapping_extra=true
CONFIG
fi

  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG

[hook_manager_params]
disable_acl_checker=true
CONFIG

if [[ -n "${DISALLOW_NON_PUSHREBASE:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[push]
pure_push_allowed = false
CONFIG
else
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[push]
pure_push_allowed = true
CONFIG
fi

if [[ -n "${COMMIT_SCRIBE_CATEGORY:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
commit_scribe_category = "$COMMIT_SCRIBE_CATEGORY"
CONFIG
fi

if [[ -n "${CACHE_WARMUP_BOOKMARK:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[cache_warmup]
bookmark="$CACHE_WARMUP_BOOKMARK"
CONFIG

  if [[ -n "${CACHE_WARMUP_MICROWAVE:-}" ]]; then
    cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
microwave_preload = true
CONFIG
  fi
fi


if [[ -n "${LFS_THRESHOLD:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[lfs]
threshold=$LFS_THRESHOLD
rollout_percentage=${LFS_ROLLOUT_PERCENTAGE:-100}
generate_lfs_blob_in_hg_sync_job=${LFS_BLOB_HG_SYNC_JOB:-true}
CONFIG
fi

write_infinitepush_config "$reponame"

cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
  [derived_data_config]
  enabled_config_name = "default"
  scuba_table = "file://$TESTTMP/derived_data_scuba.json"
CONFIG

if [[ -n "${ENABLED_DERIVED_DATA:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[derived_data_config.available_configs.default]
types = $ENABLED_DERIVED_DATA
CONFIG
else
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[derived_data_config.available_configs.default]
types=["blame", "changeset_info", "deleted_manifest", "fastlog", "filenodes", "fsnodes", "unodes", "hgchangesets", "skeleton_manifests", "bssm"]
CONFIG
fi

if [[ -n "${BLAME_VERSION}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
blame_version = $BLAME_VERSION
CONFIG
fi

if [[ -n "${HG_SET_COMMITTER_EXTRA}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
hg_set_committer_extra = true
CONFIG
fi

if [[ -n "${SEGMENTED_CHANGELOG_ENABLE:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[segmented_changelog_config]
enabled=true
heads_to_include = [
   { bookmark = "master_bookmark" },
]
skip_dag_load_at_startup=true
CONFIG
fi

if [[ -n "${BACKUP_FROM:-}" ]]; then
  cat >> "repo_definitions/$reponame_urlencoded/server.toml" <<CONFIG
backup_source_repo_name="$BACKUP_FROM"
CONFIG
fi

if [[ -n "${ENABLE_API_WRITES:-}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[source_control_service]
permit_writes = true
[[bookmarks]]
regex=".*"
hooks_skip_ancestors_of=["master_bookmark"]
CONFIG
fi

if [[ -n "${SPARSE_PROFILES_LOCATION}" ]]; then
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[sparse_profiles_config]
sparse_profiles_location="$SPARSE_PROFILES_LOCATION"
CONFIG
fi

if [[ -n "${BOOKMARK_SCRIBE_CATEGORY:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[update_logging_config]
bookmark_logging_destination = { scribe = { scribe_category = "$BOOKMARK_SCRIBE_CATEGORY" } }
CONFIG
fi
}

function write_infinitepush_config {
  local reponame="$1"
  local reponame_urlencoded
  reponame_urlencoded=$(urlencode encode "$reponame")

  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[infinitepush]
CONFIG

  if [[ -n "${INFINITEPUSH_ALLOW_WRITES:-}" ]] || \
     [[ -n "${INFINITEPUSH_NAMESPACE_REGEX:-}" ]] || \
     [[ -n "${INFINITEPUSH_HYDRATE_GETBUNDLE_RESPONSE:-}" ]];
  then
    namespace=""
    if [[ -n "${INFINITEPUSH_NAMESPACE_REGEX:-}" ]]; then
      namespace="namespace_pattern=\"$INFINITEPUSH_NAMESPACE_REGEX\""
    fi

    cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
allow_writes = ${INFINITEPUSH_ALLOW_WRITES:-true}
hydrate_getbundle_response = ${INFINITEPUSH_HYDRATE_GETBUNDLE_RESPONSE:-false}
${namespace}
CONFIG
  fi

  if [[ -n "${DRAFT_COMMIT_SCRIBE_CATEGORY:-}" ]]; then
    cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
  commit_scribe_category = "$DRAFT_COMMIT_SCRIBE_CATEGORY"
CONFIG
  fi
}

function register_hook {
  hook_name="$1"

  shift 1
  EXTRA_CONFIG_DESCRIPTOR=""
  if [[ $# -gt 0 ]]; then
    EXTRA_CONFIG_DESCRIPTOR="$1"
  fi

  reponame_urlencoded="$(urlencode encode "$REPONAME")"
  (
    cat <<CONFIG
[[bookmarks.hooks]]
hook_name="$hook_name"
[[hooks]]
name="$hook_name"
CONFIG
    [ -n "$EXTRA_CONFIG_DESCRIPTOR" ] && cat "$EXTRA_CONFIG_DESCRIPTOR"
  ) >> "repos/$reponame_urlencoded/server.toml"
}

function register_hook_limit_filesize_global_limit {
global_limit=$1
shift 1

register_hook limit_filesize <(
cat <<CONF
config_string_lists={filesize_limits_regexes=[".*"]}
config_int_lists={filesize_limits_values=[$global_limit]}
$@
CONF
)

}

function backfill_mapping {
  GLOG_minloglevel=5 "$MONONOKE_BACKFILL_MAPPING" --repo-id "$REPOID" \
  --mononoke-config-path "$TESTTMP/mononoke-config" "${COMMON_ARGS[@]}" "$@"
}

function blobimport {
  local always_log=
  if [[ "$1" == "--log" ]]; then
    always_log=1
    shift
  fi
  input="$1"
  output="$2"
  shift 2
  #   input (repo in new format)
  # --debugexportrevlog--> revlog (repo in old format)
  # --blobimport--> Mononoke repo
  local revlog="$input/revlog-export"
  rm -rf "$revlog"
  hgedenapi --cwd "$input" debugexportrevlog revlog-export
  mkdir -p "$output"
  $MONONOKE_BLOBIMPORT --repo-id $REPOID \
     --mononoke-config-path "$TESTTMP/mononoke-config" \
     "$revlog/.hg" "${COMMON_ARGS[@]}" "$@" > "$TESTTMP/blobimport.out" 2>&1
  BLOBIMPORT_RC="$?"
  if [[ $BLOBIMPORT_RC -ne 0 ]]; then
    cat "$TESTTMP/blobimport.out"
    # set exit code, otherwise previous cat sets it to 0
    return "$BLOBIMPORT_RC"
  elif [[ -n "$always_log" ]]; then
    cat "$TESTTMP/blobimport.out"
  fi
}

function bonsai_verify {
  GLOG_minloglevel=5 "$MONONOKE_BONSAI_VERIFY" --repo-id "$REPOID" \
    --mononoke-config-path "$TESTTMP/mononoke-config" "${COMMON_ARGS[@]}" "$@"
}

function manual_scrub {
  GLOG_minloglevel=5 "$MONONOKE_MANUAL_SCRUB" \
  --mononoke-config-path "$TESTTMP/mononoke-config" "${COMMON_ARGS[@]}" "$@"
}

function s_client {
    /usr/local/fbcode/platform009/bin/openssl s_client \
        -connect "$(mononoke_address)" \
        -CAfile "${TEST_CERTDIR}/root-ca.crt" \
        -cert "${TEST_CERTDIR}/client0.crt" \
        -key "${TEST_CERTDIR}/client0.key" \
        -ign_eof "$@"
}

function scs {
  SCRIBE_LOGS_DIR="$TESTTMP/scribe_logs"
  if [[ ! -d "$SCRIBE_LOGS_DIR" ]]; then
    mkdir "$SCRIBE_LOGS_DIR"
  fi

  rm -f "$TESTTMP/scs_server_addr.txt"
  GLOG_minloglevel=5 \
    THRIFT_TLS_SRV_CERT="$TEST_CERTDIR/localhost.crt" \
    THRIFT_TLS_SRV_KEY="$TEST_CERTDIR/localhost.key" \
    THRIFT_TLS_CL_CA_PATH="$TEST_CERTDIR/root-ca.crt" \
    THRIFT_TLS_TICKETS="$TEST_CERTDIR/server.pem.seeds" \
    "$SCS_SERVER" "$@" \
    --host "$LOCALIP" \
    --port 0 \
    --log-level DEBUG \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --bound-address-file "$TESTTMP/scs_server_addr.txt" \
    --scribe-logging-directory "$TESTTMP/scribe_logs" \
    "${COMMON_ARGS[@]}" >> "$TESTTMP/scs_server.out" 2>&1 &
  export SCS_SERVER_PID=$!
  echo "$SCS_SERVER_PID" >> "$DAEMON_PIDS"
}

function land_service {
  rm -f "$TESTTMP/land_service_addr.txt"
  GLOG_minloglevel=5 \
    THRIFT_TLS_SRV_CERT="$TEST_CERTDIR/localhost.crt" \
    THRIFT_TLS_SRV_KEY="$TEST_CERTDIR/localhost.key" \
    THRIFT_TLS_CL_CA_PATH="$TEST_CERTDIR/root-ca.crt" \
    THRIFT_TLS_TICKETS="$TEST_CERTDIR/server.pem.seeds" \
    "$LAND_SERVICE" "$@" \
    --host "$LOCALIP" \
    --port 0 \
    --log-level DEBUG \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --bound-address-file "$TESTTMP/land_service_addr.txt" \
    "${COMMON_ARGS[@]}" >> "$TESTTMP/land_service.out" 2>&1 &
  export LAND_SERVICE_PID=$!
  echo "$LAND_SERVICE_PID" >> "$DAEMON_PIDS"
}

function wait_for_scs {
  export SCS_PORT
  wait_for_server "SCS server" SCS_PORT "$TESTTMP/scs_server.out" \
    "${MONONOKE_SCS_START_TIMEOUT:-"$MONONOKE_SCS_DEFAULT_START_TIMEOUT"}" "$TESTTMP/scs_server_addr.txt" \
    scsc repos
}

function wait_for_land_service {
  export LAND_SERVICE_PORT
  wait_for_server "Land service" LAND_SERVICE_PORT "$TESTTMP/land_service.out" \
    "${MONONOKE_LAND_SERVICE_START_TIMEOUT:-"$MONONOKE_LAND_SERVICE_DEFAULT_START_TIMEOUT"}" "$TESTTMP/land_service_addr.txt" \
    sleep 5
}

function start_and_wait_for_scs_server {
  scs "$@"
  wait_for_scs
}

function start_and_wait_for_land_service {
  land_service "$@"
  wait_for_land_service
}

function megarepo_async_worker {
  GLOG_minloglevel=5 "$ASYNC_REQUESTS_WORKER" "$@" \
    --log-level INFO \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --scuba-dataset "file://$TESTTMP/async-worker.json" \
    "${COMMON_ARGS[@]}" >> "$TESTTMP/megarepo_async_worker.out" 2>&1 &
  export MEGAREPO_ASYNC_WORKER_PID=$!
  echo "$MEGAREPO_ASYNC_WORKER_PID" >> "$DAEMON_PIDS"
}

function scsc_as {
  local name="$1"
  shift
  GLOG_minloglevel=5 \
    THRIFT_TLS_CL_CERT_PATH="$TEST_CERTDIR/$name.crt" \
    THRIFT_TLS_CL_KEY_PATH="$TEST_CERTDIR/$name.key" \
    THRIFT_TLS_CL_CA_PATH="$TEST_CERTDIR/root-ca.crt" \
    "$SCS_CLIENT" --host "$LOCALIP:$SCS_PORT" "$@"
}

function scsc {
  scsc_as client0 "$@"
}

function lfs_health {
  local poll proto bound_addr_file
  poll="$1"; shift
  proto="$1";shift
  bound_addr_file="$1"; shift
  "$poll" "${proto}://$(cat "$bound_addr_file")/health_check"
}

function lfs_server {
  local uri log opts args proto poll lfs_server_pid bound_addr_file lfs_instance instance_count_file
  proto="http"
  poll="curl"

  # Used to separate log files etc when multiple lfs servers started in a test
  instance_count_file="$TESTTMP/lfs_instance_count.txt"

  # lfs_server is started from subshells in the .t tests
  # so use a file rather than env var to keep count
  if [[ ! -r "$instance_count_file" ]]; then
    echo 0 > "$instance_count_file"
  fi

  lfs_instance=$(($(cat "$instance_count_file") + 1))
  echo "$lfs_instance" > "$instance_count_file"

  log="${TESTTMP}/lfs_server.$lfs_instance"

  bound_addr_file="$TESTTMP/lfs_server_addr.$lfs_instance"
  rm -f "$bound_addr_file"

  opts=(
    "${COMMON_ARGS[@]}"
    --mononoke-config-path "$TESTTMP/mononoke-config"
    --listen-port 0
    --bound-address-file "$bound_addr_file"
    --test-friendly-logging
  )
  args=()

  while [[ "$#" -gt 0 ]]; do
    if [[ "$1" = "--upstream" ]]; then
      shift
      args=("${args[@]}" --upstream-url "$1") # Upstream URL is now a named parameter.
      shift
    elif [[ "$1" = "--live-config" ]]; then
      opts=("${opts[@]}" "$1" "$2") # --live-config-fetch-interval is no longer used.
      shift
      shift
    elif [[ "$1" = "--tls" ]]; then
      proto="https"
      poll="sslcurl"
      opts=(
        "${opts[@]}"
        --tls-ca "$TEST_CERTDIR/root-ca.crt"
        --tls-private-key "$TEST_CERTDIR/localhost.key"
        --tls-certificate "$TEST_CERTDIR/localhost.crt"
        --tls-ticket-seeds "$TEST_CERTDIR/server.pem.seeds"
      )
      shift
    elif
      [[ "$1" = "--always-wait-for-upstream" ]] ||
      [[ "$1" = "--readonly" ]] ||
      [[ "$1" = "--git-blob-upload-allowed" ]]
    then
      opts=("${opts[@]}" "$1")
      shift
    elif
      [[ "$1" = "--scuba-dataset" ]] ||
      [[ "$1" = "--max-upload-size" ]]
    then
      opts=("${opts[@]}" "$1" "$2")
      shift
      shift
    elif [[ "$1" = "--log" ]]; then
      shift
      log="$1"
      shift
    else
      echo "invalid argument: $1" >&2
      return 1
    fi
  done

  if [[ "$proto" = "https" ]]; then
    # need to use localhost to match test certs
    listen_host="localhost"
  else
    # use local IP as its more stable for test expectations when localhost is multihomed
    listen_host="$(mononoke_host)"
  fi
  opts=("${opts[@]}" "--listen-host" "$listen_host")

  GLOG_minloglevel=5 "$LFS_SERVER" \
    "${opts[@]}" "${args[@]}" >> "$log" 2>&1 &

  lfs_server_pid="$!"
  echo "$lfs_server_pid" >> "$DAEMON_PIDS"

  export LFS_PORT
  wait_for_server "lfs_server" "LFS_PORT" "$log" \
    "${MONONOKE_LFS_START_TIMEOUT:-"$MONONOKE_LFS_DEFAULT_START_TIMEOUT"}" "$bound_addr_file" \
    lfs_health "$poll" "$proto" "$bound_addr_file"

  export LFS_HOST_PORT
  LFS_HOST_PORT="$listen_host:$LFS_PORT"
  uri="${proto}://$LFS_HOST_PORT"
  echo "$uri"

  cp "$log" "$log.saved"
  truncate -s 0 "$log"
}

# Run an hg binary configured with the settings required to talk to Mononoke
function hgmn {
  reponame_urlencoded="$(urlencode encode "$REPONAME")"
  hg --config paths.default="mononoke://$(mononoke_address)/$reponame_urlencoded" "$@"
}

# Run an hg binary configured with the settings require to talk to Mononoke
# via EdenAPI
function hgedenapi {
  hgmn \
    --config "edenapi.url=https://localhost:$MONONOKE_SOCKET/edenapi" \
    --config "edenapi.enable=true" \
    --config "remotefilelog.http=true" \
    --config "remotefilelog.reponame=$REPONAME" \
    "$@"
}

function hginit_treemanifest() {
  hg init "$@"
  cat >> "$1"/.hg/hgrc <<EOF
[extensions]
treemanifest=!
treemanifestserver=
remotefilelog=
smartlog=
clienttelemetry=
[treemanifest]
flatcompat=False
server=True
sendtrees=True
treeonly=True
[remotefilelog]
reponame=$1
cachepath=$TESTTMP/cachepath
server=True
shallowtrees=True
[workingcopy]
ruststatus=False
use-rust=False
[status]
use-rust=False
EOF
}

function hgclone_treemanifest() {
  hg clone -q --shallow --config remotefilelog.reponame="$2" --config extensions.treemanifest= --config treemanifest.treeonly=True "$@"
  cat >> "$2"/.hg/hgrc <<EOF
[extensions]
treemanifest=
remotefilelog=
smartlog=
clienttelemetry=
[treemanifest]
flatcompat=False
sendtrees=True
treeonly=True
[remotefilelog]
reponame=$2
cachepath=$TESTTMP/cachepath
shallowtrees=True
EOF
}

function hgmn_init() {
  hg init "$@"
  cat >> "$1"/.hg/hgrc <<EOF
[extensions]
treemanifest=
remotefilelog=
remotenames=
smartlog=
clienttelemetry=
lz4revlog=
[treemanifest]
flatcompat=False
sendtrees=True
treeonly=True
[remotefilelog]
reponame=$1
cachepath=$TESTTMP/cachepath
shallowtrees=True
EOF
}

function hgmn_clone() {
  quiet hgmn clone --shallow  --config remotefilelog.reponame="$REPONAME" "$@" --config extensions.treemanifest= --config treemanifest.treeonly=True --config extensions.lz4revlog= && \
  cat >> "$2"/.hg/hgrc <<EOF
[extensions]
treemanifest=
remotefilelog=
remotenames=
smartlog=
clienttelemetry=
lz4revlog=
[treemanifest]
flatcompat=False
sendtrees=True
treeonly=True
[remotefilelog]
reponame=$REPONAME
cachepath=$TESTTMP/cachepath
shallowtrees=True
EOF
}

function enableextension() {
  cat >> .hg/hgrc <<EOF
[extensions]
$1=
EOF
}

function setup_hg_server() {
  cat >> .hg/hgrc <<EOF
[extensions]
commitextras=
treemanifest=!
treemanifestserver=
remotefilelog=
clienttelemetry=
[treemanifest]
server=True
[remotefilelog]
server=True
shallowtrees=True
EOF
}

function setup_hg_client() {
  cat >> .hg/hgrc <<EOF
[extensions]
treemanifest=
remotefilelog=
clienttelemetry=
[treemanifest]
flatcompat=False
server=False
treeonly=True
[remotefilelog]
server=False
reponame=$REPONAME
[mutation]
record=False
EOF
}

# Does all the setup necessary for hook tests
function hook_test_setup() {
  setup_mononoke_config
  cd "$TESTTMP/mononoke-config" || exit 1

  reponame_urlencoded="$(urlencode encode "$REPONAME")"
  HOOKBOOKMARK="${HOOKBOOKMARK:-master_bookmark}"
  cat >> "repos/$reponame_urlencoded/server.toml" <<CONFIG
[[bookmarks]]
name="$HOOKBOOKMARK"
CONFIG

  HOOK_NAME="$1"
  shift 1
  EXTRA_CONFIG_DESCRIPTOR=""
  if [[ $# -gt 0 ]]; then
    EXTRA_CONFIG_DESCRIPTOR="$1"
  fi


  register_hook "$HOOK_NAME" "$EXTRA_CONFIG_DESCRIPTOR"

  setup_common_hg_configs
  cd "$TESTTMP" || exit 1

  cat >> "$HGRCPATH" <<EOF
[ui]
ssh="$DUMMYSSH"
EOF

  hg init repo-hg
  cd repo-hg || exit 1
  setup_hg_server
  drawdag <<EOF
C
|
B
|
A
EOF

  hg bookmark "$HOOKBOOKMARK" -r tip

  cd ..
  blobimport repo-hg/.hg "$REPONAME"

  start_and_wait_for_mononoke_server

  hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  cd repo2 || exit 1
  setup_hg_client
  cat >> .hg/hgrc <<EOF
[extensions]
pushrebase =
remotenames =
EOF
}

function setup_hg_lfs() {
  cat >> .hg/hgrc <<EOF
[extensions]
lfs=
[lfs]
url=$1
use-client-certs=False
threshold=$2
usercache=$3
EOF
}

function setup_hg_modern_lfs() {
  cat >> .hg/hgrc <<EOF
[remotefilelog]
lfs=True
useruststore=True
getpackversion = 2
[worker]
rustworkers=True
[extensions]
lfs=!
[lfs]
use-client-certs=False
url=$1
threshold=$2
backofftimes=0
EOF
}

function setup_hg_edenapi() {
  local repo
  repo="$1"

  cat >> .hg/hgrc <<EOF
[edenapi]
enable=true
url=https://localhost:$MONONOKE_SOCKET/edenapi/$repo
[remotefilelog]
http=True
useruststore=True
getpackversion = 2
[treemanifest]
http=True
useruststore=True
[auth]
edenapi.cert=$TEST_CERTDIR/client0.crt
edenapi.key=$TEST_CERTDIR/client0.key
edenapi.prefix=localhost
edenapi.schemes=https
edenapi.cacerts=$TEST_CERTDIR/root-ca.crt
EOF
}

function aliasverify() {
  mode=$1
  shift 1
  GLOG_minloglevel=5 "$MONONOKE_ALIAS_VERIFY" --repo-id $REPOID \
     "${COMMON_ARGS[@]}" \
     --mononoke-config-path "$TESTTMP/mononoke-config" \
     --mode "$mode" "$@"
}

# Without rev
function tglogpnr() {
  hg log -G -T "{node|short} {phase} '{desc}' {bookmarks} {remotenames}" "$@"
}

function mkcommit() {
   echo "$1" > "$1"
   hg add "$1"
   hg ci -m "$1"
}

function enable_replay_verification_hook {

cat >> "$TESTTMP"/replayverification.py <<EOF
def verify_replay(ui, repo, *args, **kwargs):
     EXP_ONTO = "EXPECTED_ONTOBOOK"
     EXP_HEAD = "EXPECTED_REBASEDHEAD"
     expected_book = kwargs.get(EXP_ONTO)
     expected_head = kwargs.get(EXP_HEAD)
     actual_book = kwargs.get("key")
     actual_head = kwargs.get("new")
     allowed_replay_books = ui.configlist("facebook", "hooks.unbundlereplaybooks", [])
     # If there is a problem with the mononoke -> hg sync job we need a way to
     # quickly disable the replay verification to let unsynced bundles
     # through.
     # Disable this hook by placing a file in the .hg directory.
     if repo.localvfs.exists('REPLAY_BYPASS'):
         ui.note("[ReplayVerification] Bypassing check as override file is present\n")
         return 0
     if expected_book is None and expected_head is None:
         # We are allowing non-unbundle-replay pushes to go through
         return 0
     if allowed_replay_books and actual_book not in allowed_replay_books:
         ui.warn("[ReplayVerification] only allowed to unbundlereplay on %r\n" % (allowed_replay_books, ))
         return 1
     expected_head = expected_head or None
     actual_head = actual_head or None
     if expected_book == actual_book and expected_head == actual_head:
        ui.note("[ReplayVerification] Everything seems in order\n")
        return 0

     ui.warn("[ReplayVerification] Expected: (%r, %r). Actual: (%r, %r)\n" % (expected_book, expected_head, actual_book, actual_head))
     return 1
EOF

cat >> "$TESTTMP"/repo_lock.py << EOF
def run(ui, repo, *args, **kwargs):
   """Repo is locked for everything except replays
   In-process style hook."""
   if kwargs.get("EXPECTED_ONTOBOOK"):
       return 0
   ui.warn("[RepoLock] Repo locked for non-unbundlereplay pushes\n")
   return 1
EOF

[[ -f .hg/hgrc ]] || echo ".hg/hgrc does not exists!"

cat >>.hg/hgrc <<CONFIG
[hooks]
prepushkey = python:$TESTTMP/replayverification.py:verify_replay
prepushkey.lock = python:$TESTTMP/repo_lock.py:run
CONFIG

}

function get_bonsai_bookmark() {
  local bookmark repoid_backup
  repoid_backup="$REPOID"
  export REPOID="$1"
  bookmark="$2"
  mononoke_admin bookmarks get -c bonsai "$bookmark" 2>/dev/null | cut -d' ' -f2
  export REPOID="$repoid_backup"
}

function add_synced_commit_mapping_entry() {
  local small_repo_id large_repo_id small_bcs_id large_bcs_id version
  small_repo_id="$1"
  small_bcs_id="$2"
  large_repo_id="$3"
  large_bcs_id="$4"
  version="$5"
  mononoke_admin_source_target "$small_repo_id" "$large_repo_id" crossrepo insert rewritten --source-hash "$small_bcs_id" \
    --target-hash "$large_bcs_id" \
    --version-name "$version" 2>/dev/null
}

function read_blobstore_sync_queue_size() {
  if [[ -n "$DB_SHARD_NAME" ]]; then
    echo "SELECT COUNT(*) FROM blobstore_sync_queue;" | db "$DB_SHARD_NAME" 2> /dev/null | grep -v COUNT
  else
    local attempts timeout ret
    timeout="100"
    attempts="$((timeout * 10))"
    for _ in $(seq 1 $attempts); do
      ret="$(sqlite3 "$TESTTMP/blobstore_sync_queue/sqlite_dbs" "select count(*) from blobstore_sync_queue" 2>/dev/null)"
      if [[ -n "$ret" ]]; then
        echo "$ret"
        return 0
      fi
      sleep 0.1
    done
    return 1
  fi

}

function read_blobstore_wal_queue_size() {
  if [[ -n "$DB_SHARD_NAME" ]]; then
    echo "SELECT COUNT(*) FROM blobstore_write_ahead_log;" | db "$DB_SHARD_NAME" 2> /dev/null | grep -v COUNT
  else
    local attempts timeout ret
    timeout="100"
    attempts="$((timeout * 10))"
    for _ in $(seq 1 $attempts); do
      ret="$(sqlite3 "$TESTTMP/blobstore_sync_queue/sqlite_dbs" "select count(*) from blobstore_write_ahead_log" 2>/dev/null)"
      if [[ -n "$ret" ]]; then
        echo "$ret"
        return 0
      fi
      sleep 0.1
    done
    return 1
  fi

}

function erase_blobstore_sync_queue() {
  if [[ -n "$DB_SHARD_NAME" ]]; then
    # See above for why we have to redirect this output to /dev/null
    db -wu "$DB_SHARD_NAME" 2> /dev/null <<EOF
      DELETE FROM blobstore_sync_queue;
EOF
  else
    rm -rf "$TESTTMP/blobstore_sync_queue/sqlite_dbs"
fi
}

function log() {
  # Prepend "$" to the end of the log output to prevent having trailing whitespaces
  hg log -G -T "{desc} [{phase};rev={rev};{node|short}] {remotenames}" "$@" | sed 's/^[ \t]*$/$/'
}

# Default setup that many of the test use
function default_setup_pre_blobimport() {
  setup_common_config "$@"

  cd "$TESTTMP" || exit 1

  cat >> "$HGRCPATH" <<EOF
[ui]
ssh="$DUMMYSSH"
[extensions]
amend=
EOF

hg init repo-hg
cd repo-hg || exit 1
setup_hg_server
drawdag <<EOF
C
|
B
|
A
EOF

  hg bookmark master_bookmark -r tip

  echo "hg repo"
  log -r ":"

  cd .. || exit 1
}

function default_setup_blobimport() {
  default_setup_pre_blobimport "$@"
  echo "blobimporting"
  blobimport repo-hg/.hg "$REPONAME"
}

function default_setup() {
  default_setup_blobimport "$BLOB_TYPE"
  echo "starting Mononoke"

  start_and_wait_for_mononoke_server "$@"

  echo "cloning repo in hg client 'repo2'"
  hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  cd repo2 || exit 1
  setup_hg_client
  cat >> .hg/hgrc <<EOF
[extensions]
pushrebase =
remotenames =
EOF
}

function gitimport() {
  log="$TESTTMP/gitimport.out"

  "$MONONOKE_GITIMPORT" \
    "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function git() {
  local date name email
  date="01/01/0000 00:00 +0000"
  name="mononoke"
  email="mononoke@mononoke"
  GIT_COMMITTER_DATE="${GIT_COMMITTER_DATE:-$date}" \
  GIT_COMMITTER_NAME="$name" \
  GIT_COMMITTER_EMAIL="$email" \
  GIT_AUTHOR_DATE="${GIT_AUTHOR_DATE:-$date}" \
  GIT_AUTHOR_NAME="$name" \
  GIT_AUTHOR_EMAIL="$email" \
  command git "$@"
}

function git_set_only_author() {
  local date name email
  date="01/01/0000 00:00 +0000"
  name="mononoke"
  email="mononoke@mononoke"
  GIT_AUTHOR_DATE="$date" \
  GIT_AUTHOR_NAME="$name" \
  GIT_AUTHOR_EMAIL="$email" \
  command git "$@"
}

function summarize_scuba_json() {
  local interesting_tags
  local key_spec
  interesting_tags="$1"
  shift
  key_spec=""
  for key in "$@"
  do
     key_spec="$key_spec + (if (${key} != null) then {${key##*.}: ${key}} else {} end)"
  done
  jq -S "if (.normal.log_tag | match(\"^($interesting_tags)\$\")) then ${key_spec:3} else empty end"
}

if [ -z "$HAS_FB" ]; then
  function format_single_scuba_sample() {
    jq -S .
  }

  function format_single_scuba_sample_strip_server_info {
      jq -S 'del(.[].server_tier, .[].tw_task_id, .[].tw_handle)'
  }
fi

function regenerate_hg_filenodes() {
  "$MONONOKE_REGENERATE_HG_FILENODES" \
    "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    --i-know-what-i-am-doing \
    "$@"
}

function segmented_changelog_tailer_reseed() {
  "$MONONOKE_SEGMENTED_CHANGELOG_TAILER" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    --force-reseed \
    "$@"
}

function segmented_changelog_tailer_once() {
  "$MONONOKE_SEGMENTED_CHANGELOG_TAILER" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    --once \
    "$@"
}

function background_segmented_changelog_tailer() {
  out_file=$1
  shift
  # short delay here - we don't want to wait too much during tests
  "$MONONOKE_SEGMENTED_CHANGELOG_TAILER" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@" >> "$TESTTMP/$out_file" 2>&1 &
  pid=$!
  echo "$pid" >> "$DAEMON_PIDS"
}

function microwave_builder() {
  "$MONONOKE_MICROWAVE_BUILDER" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function backfill_derived_data() {
  "$MONONOKE_BACKFILL_DERIVED_DATA" \
    --debug \
    "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function backfill_derived_data_multiple_repos() {
  IFS=':' read -r -a ids <<< "${REPOS[*]}"
  "$MONONOKE_BACKFILL_DERIVED_DATA" \
    "${COMMON_ARGS[@]}" \
    "${ids[@]}" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function hook_tailer() {
  "$MONONOKE_HOOK_TAILER" \
    "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function quiet() {
  local log="$TESTTMP/quiet.last.log"
  "$@" >"$log" 2>&1
  ret="$?"
  if [[ "$ret" == 0 ]]; then
    return "$ret"
  fi
  cat "$log"
  return "$ret"
}

function copy_blobstore_keys() {
  SOURCE_REPO_ID="$1"
  shift
  TARGET_REPO_ID="$1"
  shift

  GLOG_minloglevel=5 "$COPY_BLOBSTORE_KEYS" "${COMMON_ARGS[@]}" \
    --source-repo-id "$SOURCE_REPO_ID" \
    --target-repo-id "$TARGET_REPO_ID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function streaming_clone() {
  GLOG_minloglevel=5 "$MONONOKE_STREAMING_CLONE" "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function repo_import() {
  log="$TESTTMP/repo_import.out"

  "$MONONOKE_REPO_IMPORT" \
    "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function sqlite3() {
  # Set a longer timeout so that we don't break if Mononoke currently has a
  # handle on the DB.
  command sqlite3 -cmd '.timeout 1000' "$@"
}

function merge_tunables() {
  local new
  new="$(jq -s '.[0] * .[1]' "$MONONOKE_TUNABLES_PATH" -)"
  printf "%s" "$new" > "$MONONOKE_TUNABLES_PATH"
  # This may fail if Mononoke is not started. No big deal.
  force_update_configerator >/dev/null 2>&1 || true
}

function init_tunables() {
  if [[ ! -f "$MONONOKE_TUNABLES_PATH" ]]; then
    cat >> "$MONONOKE_TUNABLES_PATH" <<EOF
{}
EOF
  fi
}

# Always initialize tunables, since they're required by our binaries to start
# unless explicitly disabled (but we don't do that in tests).
init_tunables

function packer() {
  "$MONONOKE_PACKER" \
    "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function check_git_wc() {
  "$MONONOKE_CHECK_GIT_WC" \
    "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function derived_data_service() {
  export PORT_2DS
  local DDS_SERVER_ADDR_FILE
  DDS_SERVER_ADDR_FILE="$TESTTMP/dds_server_addr.txt"
  GLOG_minloglevel=5 "$DERIVED_DATA_SERVICE" "$@" \
    -p 0 \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    --bound-address-file "$DDS_SERVER_ADDR_FILE" \
    "${COMMON_ARGS[@]}" >> "$TESTTMP/derived_data_service.out" 2>&1 &

  pid=$!
  echo "$pid" >> "$DAEMON_PIDS"

  wait_for_server "Derived data service" PORT_2DS "$TESTTMP/derived_data_service.out" \
    "${MONONOKE_DDS_START_TIMEOUT:-"$MONONOKE_DDS_DEFAULT_START_TIMEOUT"}" "$DDS_SERVER_ADDR_FILE" \
    derived_data_client get-status
}

function derived_data_client() {
  GLOG_minloglevel=5 "$DERIVED_DATA_CLIENT" \
  -h "localhost:$PORT_2DS" \
  "$@"
}

function derivation_worker() {
  GLOG_minloglevel=5 "$DERIVED_DATA_WORKER" "${COMMON_ARGS[@]}" --mononoke-config-path "$TESTTMP"/mononoke-config "$@"
}

function verify_integrity_service_health() {
  $THRIFTDBG sendRequest getStatus "{}" --host "localhost" --port "$VI_SERVICE_PORT"
}

function verify_integrity_service() {
  export VI_SERVICE_PORT
  local VI_SERVICE_ADDR_FILE
  VI_SERVICE_ADDR_FILE="$TESTTMP/verify_integrity_service_addr.txt"
  "$VERIFY_INTEGRITY_SERVICE" "$@" \
    --service.port 0 \
    --bound-address-file "$VI_SERVICE_ADDR_FILE" \
    >> "$TESTTMP/verify_integrity_service.out" 2>&1 &

  pid=$!
  echo "$pid" >> "$DAEMON_PIDS"

  wait_for_server "Verify Integrity service" VI_SERVICE_PORT "$TESTTMP/verify_integrity_service.out" \
    "${VI_SERVICE_START_TIMEOUT:-"$VI_SERVICE_DEFAULT_START_TIMEOUT"}" "$VI_SERVICE_ADDR_FILE" \
    verify_integrity_service_health
}

# Wrapper for drawdag that loads all the commit aliases to env variables
# so they can be used to refer to commits instead of hashes.
function testtool_drawdag() {
  out="$(mononoke_testtool drawdag "$@" | tee /dev/fd/2)"
  rc="${PIPESTATUS[0]}"
  # shellcheck disable=SC2046,SC2163,SC2086
  export $out
  return "$rc"
}

function start_zelos_server() {
  PORT=$1
  rm -f "$TESTTMP/local-zelos"
  "$ZELOSCLI" --x server "$PORT" "$TESTTMP/local-zelos" > /dev/null 2>&1 &
  pid=$!
  echo "$pid" >> "$DAEMON_PIDS"
}

function zeloscli() {
  PORT=$1
  shift
  "$ZELOSCLI" --server localhost:"$PORT" -x "$@" 2>/dev/null
}
