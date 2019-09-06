#!/bin/bash
# Copyright (c) 2019-present, Facebook, Inc.
# All Rights Reserved.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# Library routines and initial setup for Mononoke-related tests.

if [[ -n "$DB_SHARD_NAME" ]]; then
  function db_config() {
    echo "db.db_address=\"$DB_SHARD_NAME\""
  }
  MONONOKE_DEFAULT_START_TIMEOUT=60
else
  function db_config() {
    local reponame
    reponame="$1"
    echo "db.local_db_path=\"$TESTTMP/$reponame\""
  }
  MONONOKE_DEFAULT_START_TIMEOUT=15
fi

CACHING_ARGS=(--skip-caching)
TEST_CERTDIR="${HGTEST_CERTDIR:-"$TEST_CERTS"}"

function next_repo_id {
    if [[ -e "$TESTTMP/mononoke-config/last_id" ]] ; then
        LAST_ID=$(< "$TESTTMP/mononoke-config/last_id")
    else
        LAST_ID=-1
    fi
    NEW_ID=$(( LAST_ID + 1 ))
    printf $NEW_ID
    printf $NEW_ID > "$TESTTMP/mononoke-config/last_id"
}

function get_free_socket {

# From https://unix.stackexchange.com/questions/55913/whats-the-easiest-way-to-find-an-unused-local-port
  cat > "$TESTTMP/get_free_socket.py" <<EOF
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.bind(('', 0))
addr = s.getsockname()
print(addr[1])
s.close()
EOF

  python "$TESTTMP/get_free_socket.py"
}

# return random value from [1, max_value]
function random_int() {
  max_value=$1

  VAL=$RANDOM
  (( VAL %= $max_value ))
  (( VAL += 1 ))

  echo $VAL
}

function sslcurl {
  curl --cert "$TEST_CERTDIR/localhost.crt" --cacert "$TEST_CERTDIR/root-ca.crt" --key "$TEST_CERTDIR/localhost.key" "$@"
}

function mononoke {
  # Ignore specific Python warnings to make tests predictable.
  export MONONOKE_SOCKET
  MONONOKE_SOCKET=$(get_free_socket)
  PYTHONWARNINGS="ignore:::requests" \
  "$MONONOKE_SERVER" "$@" --ca-pem "$TEST_CERTDIR/root-ca.crt" \
  --private-key "$TEST_CERTDIR/localhost.key" \
  --cert "$TEST_CERTDIR/localhost.crt" \
  --ssl-ticket-seeds "$TEST_CERTDIR/server.pem.seeds" \
  --debug \
  --test-instance \
  --listening-host-port "[::1]:$MONONOKE_SOCKET" \
  -P "$TESTTMP/mononoke-config" \
   "${CACHING_ARGS[@]}" >> "$TESTTMP/mononoke.out" 2>&1 &
  export MONONOKE_PID=$!
  echo "$MONONOKE_PID" >> "$DAEMON_PIDS"
}

function mononoke_hg_sync {
  $MONONOKE_HG_SYNC \
    "${CACHING_ARGS[@]}" \
    --retry-num 1 \
    --repo-id 0 \
    --mononoke-config-path mononoke-config  \
    --verify-server-bookmark-on-failure \
     ssh://user@dummy/"$1" sync-once --start-id "$2"
}

function megarepo_tool {
  $MEGAREPO_TOOL \
    "${CACHING_ARGS[@]}" \
    --repo-id 0 \
    --mononoke-config-path mononoke-config  \
    "$@"
}

function mononoke_rechunker {
    "$MONONOKE_RECHUNKER" \
    "${CACHING_ARGS[@]}" \
    --repo-id 0 \
    --mononoke-config-path mononoke-config \
    "$@"
}

function mononoke_hg_sync_with_retry {
  $MONONOKE_HG_SYNC \
    "${CACHING_ARGS[@]}" \
    --base-retry-delay-ms 1 \
    --repo-id 0 \
    --mononoke-config-path mononoke-config  \
    --verify-server-bookmark-on-failure \
     ssh://user@dummy/"$1" sync-once --start-id "$2"
}

function mononoke_hg_sync_with_failure_handler {
  sql_name="${TESTTMP}/hgrepos/repo_lock"

  $MONONOKE_HG_SYNC \
    "${CACHING_ARGS[@]}" \
    --retry-num 1 \
    --repo-id 0 \
    --mononoke-config-path mononoke-config  \
    --verify-server-bookmark-on-failure \
    --lock-on-failure \
    --repo-lock-sqlite \
    --repo-lock-db-address "$sql_name" \
     ssh://user@dummy/"$1" sync-once --start-id "$2"
}

function create_repo_lock_sqlite3_db {
  cat >> "$TESTTMP"/repo_lock.sql <<SQL
  CREATE TABLE repo_lock (
    repo VARCHAR(255) PRIMARY KEY,
    state INTEGER NOT NULL,
    reason VARCHAR(255)
  );
SQL
  mkdir -p "$TESTTMP"/hgrepos
  sqlite3 "$TESTTMP/hgrepos/repo_lock" < "$TESTTMP"/repo_lock.sql
}

function init_repo_lock_sqlite3_db {
  # State 2 is mononoke write
  sqlite3 "$TESTTMP/hgrepos/repo_lock" \
    "insert into repo_lock (repo, state, reason) values(CAST('repo' AS BLOB), 2, null)";
}

function mononoke_bookmarks_filler {
  local sql_source sql_name

  if [[ -n "$DB_SHARD_NAME" ]]; then
    sql_source="xdb"
    sql_name="$DB_SHARD_NAME"
  else
    sql_source="sqlite"
    sql_name="${TESTTMP}/replaybookmarksqueue"
  fi

  "$MONONOKE_BOOKMARKS_FILLER" \
    "${CACHING_ARGS[@]}" \
    --repo-id 0 \
    --mononoke-config-path mononoke-config  \
    "$@" "$sql_source" "$sql_name"
}

function create_mutable_counters_sqlite3_db {
  cat >> "$TESTTMP"/mutable_counters.sql <<SQL
  CREATE TABLE mutable_counters (
    repo_id INT UNSIGNED NOT NULL,
    name VARCHAR(512) NOT NULL,
    value BIGINT NOT NULL,
    PRIMARY KEY (repo_id, name)
  );
SQL
  sqlite3 "$TESTTMP/repo/mutable_counters" < "$TESTTMP"/mutable_counters.sql
}

function init_mutable_counters_sqlite3_db {
  sqlite3 "$TESTTMP/repo/mutable_counters" \
  "insert into mutable_counters (repo_id, name, value) values(0, 'latest-replayed-request', 0)";
}

function create_books_sqlite3_db {
  cat >> "$TESTTMP"/bookmarks.sql <<SQL
  CREATE TABLE bookmarks_update_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
  repo_id INT UNSIGNED NOT NULL,
  name VARCHAR(512) NOT NULL,
  from_changeset_id VARBINARY(32),
  to_changeset_id VARBINARY(32),
  reason VARCHAR(32) NOT NULL, -- enum is used in mysql
  timestamp BIGINT NOT NULL
);
SQL

  sqlite3 "$TESTTMP/repo/bookmarks" < "$TESTTMP"/bookmarks.sql
}

function mononoke_hg_sync_loop {
  local repo="$1"
  local start_id="$2"
  shift
  shift

  $MONONOKE_HG_SYNC \
    "${CACHING_ARGS[@]}" \
    --retry-num 1 \
    --repo-id 0 \
    --mononoke-config-path mononoke-config \
    ssh://user@dummy/"$repo" sync-loop --start-id "$start_id" "$@"
}

function mononoke_admin {
  "$MONONOKE_ADMIN" \
    "${CACHING_ARGS[@]}" \
    --repo-id 0 \
    --mononoke-config-path "$TESTTMP"/mononoke-config "$@"
}

function write_stub_log_entry {
  "$WRITE_STUB_LOG_ENTRY" \
    "${CACHING_ARGS[@]}" \
    --repo-id 0 \
    --mononoke-config-path "$TESTTMP"/mononoke-config --bookmark master_bookmark "$@"
}

# Wait until a Mononoke server is available for this repo.
function wait_for_mononoke {
  # MONONOKE_START_TIMEOUT is set in seconds
  # Number of attempts is timeout multiplied by 10, since we
  # sleep every 0.1 seconds.
  local attempts timeout
  timeout="${MONONOKE_START_TIMEOUT:-"$MONONOKE_DEFAULT_START_TIMEOUT"}"
  attempts="$((timeout * 10))"

  SSLCURL="sslcurl --noproxy localhost \
                https://localhost:$MONONOKE_SOCKET"

  for _ in $(seq 1 $attempts); do
    $SSLCURL 2>&1 | grep -q 'Empty reply' && break
    sleep 0.1
  done

  if ! $SSLCURL 2>&1 | grep -q 'Empty reply'; then
    echo "Mononoke did not start" >&2
    cat "$TESTTMP/mononoke.out"
    exit 1
  fi
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
[extensions]
remotefilelog=
[remotefilelog]
cachepath=$TESTTMP/cachepath
EOF
}

function setup_common_config {
    setup_mononoke_config "$@"
    setup_common_hg_configs
}

function create_pushrebaserecording_sqlite3_db {
  cat >> "$TESTTMP"/pushrebaserecording.sql <<SQL
  CREATE TABLE pushrebaserecording (
     id bigint(20) NOT NULL,
     repo_id int(10) NOT NULL,
     ontorev binary(40) NOT NULL,
     onto varchar(512) NOT NULL,
     onto_rebased_rev binary(40),
     conflicts longtext,
     pushrebase_errmsg varchar(1024) DEFAULT NULL,
     upload_errmsg varchar(1024) DEFAULT NULL,
     bundlehandle varchar(1024) DEFAULT NULL,
     timestamps longtext NOT NULL,
     recorded_manifest_hashes longtext NOT NULL,
     real_manifest_hashes longtext NOT NULL,
     duration_ms int(10) DEFAULT NULL,
     replacements_revs varchar(1024) DEFAULT NULL,
     ordered_added_revs varchar(1024) DEFAULT NULL,
    PRIMARY KEY (id)
  );
SQL
  sqlite3 "$TESTTMP"/pushrebaserecording < "$TESTTMP"/pushrebaserecording.sql
}

function init_pushrebaserecording_sqlite3_db {
  sqlite3 "$TESTTMP/pushrebaserecording" \
  "insert into pushrebaserecording \
  (id, repo_id, bundlehandle, ontorev, onto, timestamps, recorded_manifest_hashes, real_manifest_hashes)  \
  values(1, 0, 'handle', 'add0c792bfce89610d277fd5b1e32f5287994d1d', 'master_bookmark', '', '', '')";
}

function init_bookmark_log_sqlite3_db {
  sqlite3 "$TESTTMP/repo/bookmarks" \
  "insert into bookmarks_update_log \
  (repo_id, name, from_changeset_id, to_changeset_id, reason, timestamp) \
  values(0, 'master_bookmark', NULL, X'04C1EA537B01FFF207445E043E310807F9059572DD3087A0FCE458DEC005E4BD', 'pushrebase', 0)";

  sqlite3 "$TESTTMP/repo/bookmarks" "select * from bookmarks_update_log";
}

function setup_mononoke_config {
  cd "$TESTTMP" || exit
  mkdir mononoke-config
  REPOTYPE="blob:rocks"
  if [[ $# -gt 0 ]]; then
    REPOTYPE="$1"
  fi

  if [[ ! -e "$TESTTMP/mononoke_hgcli" ]]; then
    cat >> "$TESTTMP/mononoke_hgcli" <<EOF
#! /bin/sh
"$MONONOKE_HGCLI" --no-session-output "\$@"
EOF
    chmod a+x "$TESTTMP/mononoke_hgcli"
    MONONOKE_HGCLI="$TESTTMP/mononoke_hgcli"
  fi

  ALLOWED_USERNAME="${ALLOWED_USERNAME:-myusername0}"

  cd mononoke-config
  mkdir -p common
  cat > common/common.toml <<CONFIG
[[whitelist_entry]]
identity_type = "USER"
identity_data = "$ALLOWED_USERNAME"
CONFIG

  setup_mononoke_repo_config repo

  mkdir -p repos/disabled_repo
  cat > repos/disabled_repo/server.toml <<CONFIG
repoid=$(next_repo_id)
enabled=false
storage_config = "blobstore"

[storage.blobstore]
blobstore_type = "$REPOTYPE"
path = "$TESTTMP/disabled_repo"
db.local_db_path = "$TESTTMP/disabled_repo"
CONFIG
}

function setup_mononoke_repo_config {
  cd "$TESTTMP/mononoke-config" || exit
  local reponame="$1"
  mkdir -p "repos/$reponame"
  mkdir -p "$TESTTMP/$reponame"
  mkdir -p "$TESTTMP/$reponame/blobs"
  cat > "repos/$reponame/server.toml" <<CONFIG
repoid=$(next_repo_id)
enabled=true
hash_validation_percentage=100
bookmarks_cache_ttl=2000
storage_config = "blobstore"
CONFIG

if [[ -v READ_ONLY_REPO ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
readonly=true
CONFIG
fi

if [[ -v FILESTORE ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[filestore]
chunk_size = ${FILESTORE_CHUNK_SIZE:-10}
concurrency = 24
CONFIG
fi

if [[ -v REDACTION_DISABLED ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
redaction=false
CONFIG
fi

if [[ -v LIST_KEYS_PATTERNS_MAX ]]; then
  cat >> repos/repo/server.toml <<CONFIG
list_keys_patterns_max=$LIST_KEYS_PATTERNS_MAX
CONFIG
fi

if [[ -v MULTIPLEXED ]]; then
cat >> "repos/$reponame/server.toml" <<CONFIG
[storage.blobstore]
$(db_config "$reponame")
blobstore_type="multiplexed"

    [[storage.blobstore.components]]
    blobstore_id=0
    blobstore_type="blob:files"
    path = "$TESTTMP/$reponame/0"

    [[storage.blobstore.components]]
    blobstore_id=1
    blobstore_type="blob:files"
    path = "$TESTTMP/$reponame/1"
CONFIG
else
  cat >> "repos/$reponame/server.toml" <<CONFIG
[storage.blobstore]
$(db_config "$reponame")
blobstore_type = "$REPOTYPE"
path = "$TESTTMP/$reponame"

CONFIG
fi

if [[ -v ONLY_FAST_FORWARD_BOOKMARK ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[[bookmarks]]
name="$ONLY_FAST_FORWARD_BOOKMARK"
only_fast_forward=true
CONFIG
fi

if [[ -v ONLY_FAST_FORWARD_BOOKMARK_REGEX ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[[bookmarks]]
regex="$ONLY_FAST_FORWARD_BOOKMARK_REGEX"
only_fast_forward=true
CONFIG
fi

  cat >> "repos/$reponame/server.toml" <<CONFIG
[pushrebase]
forbid_p2_root_rebases=false
CONFIG

if [[ -v BLOCK_MERGES ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
block_merges=true
CONFIG
fi

if [[ -v PUSHREBASE_REWRITE_DATES ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
rewritedates=true
CONFIG
else
  cat >> "repos/$reponame/server.toml" <<CONFIG
rewritedates=false
CONFIG
fi

if [[ -v EMIT_OBSMARKERS ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
emit_obsmarkers=true
CONFIG
fi

if [[ ! -v ENABLE_ACL_CHECKER ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[hook_manager_params]
entrylimit=1048576
weightlimit=104857600
disable_acl_checker=true
CONFIG
fi

if [[ -v ENABLE_PRESERVE_BUNDLE2 ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[bundle2_replay_params]
preserve_raw_bundle2 = true
CONFIG
fi

if [[ -v DISALLOW_NON_PUSHREBASE ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[push]
pure_push_allowed = false
CONFIG
fi

if [[ -v CACHE_WARMUP_BOOKMARK ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[cache_warmup]
bookmark="$CACHE_WARMUP_BOOKMARK"
CONFIG
fi

if [[ -v LFS_THRESHOLD ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[lfs]
threshold=$LFS_THRESHOLD
CONFIG
fi

if [[ -v INFINITEPUSH_ALLOW_WRITES ]] || [[ -v INFINITEPUSH_NAMESPACE_REGEX ]]; then
  namespace=""
  if [[ -v INFINITEPUSH_NAMESPACE_REGEX ]]; then
    namespace="namespace=\"$INFINITEPUSH_NAMESPACE_REGEX\""
  fi

  cat >> repos/repo/server.toml <<CONFIG
[infinitepush]
allow_writes = ${INFINITEPUSH_ALLOW_WRITES:-true}
${namespace}
CONFIG
fi
}

function register_hook {
  hook_name="$1"
  path="$2"
  hook_type="$3"

  shift 3
  EXTRA_CONFIG_DESCRIPTOR=""
  if [[ $# -gt 0 ]]; then
    EXTRA_CONFIG_DESCRIPTOR="$1"
  fi

  (
    cat <<CONFIG
[[bookmarks.hooks]]
hook_name="$hook_name"
[[hooks]]
name="$hook_name"
path="$path"
hook_type="$hook_type"
CONFIG
    [ -n "$EXTRA_CONFIG_DESCRIPTOR" ] && cat "$EXTRA_CONFIG_DESCRIPTOR"
  ) >> repos/repo/server.toml
}

function blobimport {
  input="$1"
  output="$2"
  shift 2
  mkdir -p "$output"
  $MONONOKE_BLOBIMPORT --repo_id 0 \
     --mononoke-config-path "$TESTTMP/mononoke-config" \
     "$input" "${CACHING_ARGS[@]}" "$@" >> "$TESTTMP/blobimport.out" 2>&1
  BLOBIMPORT_RC="$?"
  if [[ $BLOBIMPORT_RC -ne 0 ]]; then
    cat "$TESTTMP/blobimport.out"
    # set exit code, otherwise previous cat sets it to 0
    return "$BLOBIMPORT_RC"
  fi
}

function bonsai_verify {
  GLOG_minloglevel=2 $MONONOKE_BONSAI_VERIFY --repo_id 0 \
  --mononoke-config-path "$TESTTMP/mononoke-config" "${CACHING_ARGS[@]}" "$@"
}

function lfs_import {
  "$MONONOKE_LFS_IMPORT" --repo_id 0 \
  --mononoke-config-path "$TESTTMP/mononoke-config" "${CACHING_ARGS[@]}" "$@"
}

function setup_no_ssl_apiserver {
  APISERVER_PORT=$(get_free_socket)
  no_ssl_apiserver --http-host "127.0.0.1" --http-port "$APISERVER_PORT"
  wait_for_apiserver --no-ssl
}


function apiserver {
  "$MONONOKE_APISERVER" "$@" --mononoke-config-path "$TESTTMP/mononoke-config" \
   --without-skiplist \
    --ssl-ca "$TEST_CERTDIR/root-ca.crt" \
    --ssl-private-key "$TEST_CERTDIR/localhost.key" \
    --ssl-certificate "$TEST_CERTDIR/localhost.crt" \
    --ssl-ticket-seeds "$TEST_CERTDIR/server.pem.seeds" \
    "${CACHING_ARGS[@]}" >> "$TESTTMP/apiserver.out" 2>&1 &
  export APISERVER_PID=$!
  echo "$APISERVER_PID" >> "$DAEMON_PIDS"
}

function no_ssl_apiserver {
  "$MONONOKE_APISERVER" "$@" \
   --without-skiplist \
   --mononoke-config-path "$TESTTMP/mononoke-config" \
   "${CACHING_ARGS[@]}" >> "$TESTTMP/apiserver.out" 2>&1 &
  echo $! >> "$DAEMON_PIDS"
}

function wait_for_apiserver {
  for _ in $(seq 1 200); do
    if [[ -a "$TESTTMP/apiserver.out" ]]; then
      PORT=$(grep "Listening to" < "$TESTTMP/apiserver.out" | grep -Pzo "(\\d+)\$") && break
    fi
    sleep 0.1
  done

  if [[ -z "$PORT" ]]; then
    echo "error: Mononoke API Server is not started"
    cat "$TESTTMP/apiserver.out"
    exit 1
  fi

  export APIHOST="localhost:$PORT"
  export APISERVER
  APISERVER="https://localhost:$PORT"
  if [[ ($# -eq 1 && "$1" == "--no-ssl") ]]; then
    APISERVER="http://localhost:$PORT"
  fi
}

function extract_json_error {
  input=$(< /dev/stdin)
  echo "$input" | head -1 | jq -r '.message'
  echo "$input" | tail -n +2
}

# Run an hg binary configured with the settings required to talk to Mononoke.
function hgmn {
  hg --config ui.ssh="$DUMMYSSH" --config paths.default=ssh://user@dummy/repo --config ui.remotecmd="$MONONOKE_HGCLI" "$@"
}

function hgmn_show {
  echo "LOG $*"
  hgmn log --template 'node:\t{node}\np1node:\t{p1node}\np2node:\t{p2node}\nauthor:\t{author}\ndate:\t{date}\ndesc:\t{desc}\n\n{diff()}' -r "$@"
  hgmn update "$@"
  echo
  echo "CONTENT $*"
  find . -type f -not -path "./.hg/*" -print -exec cat {} \;
}

function hginit_treemanifest() {
  hg init "$@"
  cat >> "$1"/.hg/hgrc <<EOF
[extensions]
treemanifest=
fastmanifest=!
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
EOF
}

function hgclone_treemanifest() {
  hg clone -q --shallow --config remotefilelog.reponame=master "$@" --config extensions.treemanifest= --config treemanifest.treeonly=True
  cat >> "$2"/.hg/hgrc <<EOF
[extensions]
treemanifest=
remotefilelog=
fastmanifest=
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

function enableextension() {
  cat >> .hg/hgrc <<EOF
[extensions]
$1=
EOF
}

function setup_hg_server() {
  cat >> .hg/hgrc <<EOF
[extensions]
treemanifest=
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
reponame=repo
EOF
}

# Does all the setup necessary for hook tests
function hook_test_setup() {
  # shellcheck source=fbcode/scm/mononoke/tests/integration/library.sh
  . "${TEST_FIXTURES}/library.sh"

  setup_mononoke_config
  cd "$TESTTMP/mononoke-config" || exit 1

  HOOKBOOKMARK="${HOOKBOOKMARK:-master_bookmark}"
  cat >> repos/repo/server.toml <<CONFIG
[[bookmarks]]
name="$HOOKBOOKMARK"
CONFIG

  HOOK_FILE="$1"
  HOOK_NAME="$2"
  HOOK_TYPE="$3"
  shift 3
  EXTRA_CONFIG_DESCRIPTOR=""
  if [[ $# -gt 0 ]]; then
    EXTRA_CONFIG_DESCRIPTOR="$1"
  fi


  if [[ ! -z "$HOOK_FILE" ]] ; then
    mkdir -p common/hooks
    cp "$HOOK_FILE" common/hooks/"$HOOK_NAME".lua
    register_hook "$HOOK_NAME" common/hooks/"$HOOK_NAME".lua "$HOOK_TYPE" "$EXTRA_CONFIG_DESCRIPTOR"
  else
    register_hook "$HOOK_NAME" "" "$HOOK_TYPE" "$EXTRA_CONFIG_DESCRIPTOR"
  fi

  setup_common_hg_configs
  cd "$TESTTMP" || exit 1

  cat >> "$HGRCPATH" <<EOF
[ui]
ssh="$DUMMYSSH"
EOF

  hg init repo-hg
  cd repo-hg || exit 1
  setup_hg_server
  hg debugdrawdag <<EOF
C
|
B
|
A
EOF

  hg bookmark "$HOOKBOOKMARK" -r tip

  cd ..
  blobimport repo-hg/.hg repo

  mononoke
  wait_for_mononoke "$TESTTMP"/repo

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
threshold=$2
usercache=$3
EOF
}

function aliasverify() {
  mode=$1
  shift 1
  GLOG_minloglevel=2 $MONONOKE_ALIAS_VERIFY --repo_id 0 \
     "${CACHING_ARGS[@]}" \
     --mononoke-config-path "$TESTTMP/mononoke-config" \
     --mode "$mode" "$@"
}

# Without rev
function tglogpnr() {
  hg log -G -T "{node|short} {phase} '{desc}' {bookmarks} {branches}" "$@"
}

function mkcommit() {
   echo "$1" > "$1"
   hg add "$1"
   hg ci -m "$1"
}

function pushrebase_replay() {
  DB_INDICES=$1

  REPLAY_CA_PEM="$TEST_CERTDIR/root-ca.crt" \
  THRIFT_TLS_CL_CERT_PATH="$TEST_CERTDIR/localhost.crt" \
  THRIFT_TLS_CL_KEY_PATH="$TEST_CERTDIR/localhost.key" \
  "$PUSHREBASE_REPLAY" \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --reponame repo \
    --hgcli "$MONONOKE_HGCLI" \
    --mononoke-admin "$MONONOKE_ADMIN" \
    --mononoke-address "[::1]:$MONONOKE_SOCKET" \
    --mononoke-server-common-name localhost \
    --db-indices "$DB_INDICES" \
    --repoid 0 \
    --bundle-provider filesystem \
    --filesystem-bundles-storage-path "$TESTTMP" \
    --sqlite3-path "$TESTTMP/pushrebaserecording" \
    "${CACHING_ARGS[@]}"
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

     ui.warn("[ReplayVerification] Expected: (%s, %s). Actual: (%s, %s)\n" % (expected_book, expected_head, actual_book, actual_head))
     return 1
EOF

cat >> "$TESTTMP"/repo_lock.py << EOF
def run(*args, **kwargs):
   """Repo is locked for everything except replays
   In-process style hook."""
   if kwargs.get("EXPECTED_ONTOBOOK"):
       return 0
   print "[RepoLock] Repo locked for non-unbundlereplay pushes"
   return 1
EOF

[[ -f .hg/hgrc ]] || echo ".hg/hgrc does not exists!"

cat >>.hg/hgrc <<CONFIG
[hooks]
prepushkey = python:$TESTTMP/replayverification.py:verify_replay
prepushkey.lock = python:$TESTTMP/repo_lock.py:run
CONFIG

}

# We only have one testing shard available, which is patterned off of the
# Mononoke DB. However, we don't have this particular table in there (since
# we're connecting to commit cloud for it). Note that we have to silence output
# from the db tool: it's a bit chatty in unpredictable ways.
function create_replaybookmarks_table() {
  if [[ -n "$DB_SHARD_NAME" ]]; then
    db -w "$DB_SHARD_NAME" 2>/dev/null  <<EOF
    CREATE TABLE replaybookmarksqueue (
  id BIGINT(20) UNSIGNED NOT NULL AUTO_INCREMENT,
  reponame varbinary(255) NOT NULL,
  bookmark varbinary(512) NOT NULL,
  node varbinary(64) NOT NULL,
  bookmark_hash varbinary(64) NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  synced TINYINT(1) NOT NULL DEFAULT 0,
  backfill TINYINT(1) NOT NULL DEFAULT 0,
  PRIMARY KEY (id),
  KEY sync_queue (synced, reponame, bookmark_hash)
) ENGINE=InnoDB DEFAULT CHARSET=utf8;
EOF
else
  # We don't actually create any DB here, replaybookmarks will create it for it
  # when it opens a SQLite DB in this directory.
  mkdir "$TESTTMP/replaybookmarksqueue"
fi
}

function insert_replaybookmarks_entry() {
  local repo bookmark
  repo="$1"
  bookmark="$2"
  node="$3"

  if [[ -n "$DB_SHARD_NAME" ]]; then
    # See above for why we have to redirect this output to /dev/null
    db -w "$DB_SHARD_NAME" 2>/dev/null <<EOF
      INSERT INTO replaybookmarksqueue (reponame, bookmark, node, bookmark_hash)
      VALUES ('$repo', '$bookmark', '$node', '$bookmark');
EOF
  else
    sqlite3 "$TESTTMP/replaybookmarksqueue/replaybookmarksqueue" <<EOF
      INSERT INTO replaybookmarksqueue (reponame, bookmark, node, bookmark_hash)
      VALUES (CAST('$repo' AS BLOB), '$bookmark', '$node', '$bookmark');
EOF
fi
}
