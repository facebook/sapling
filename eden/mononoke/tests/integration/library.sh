#!/bin/bash
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Library routines and initial setup for Mononoke-related tests.

if [ -f "$TEST_FIXTURES/facebook/fb_library.sh" ]; then
  # shellcheck source=fbcode/eden/mononoke/tests/integration/facebook/fb_library.sh
  . "$TEST_FIXTURES/facebook/fb_library.sh"
fi

ALLOWED_IDENTITY_TYPE="${FB_ALLOWED_IDENTITY_TYPE:-X509_SUBJECT_NAME}"
ALLOWED_IDENTITY_DATA="${FB_ALLOWED_IDENTITY_DATA:-CN=localhost,O=Mononoke,C=US,ST=CA}"
JSON_CLIENT_ID="${FB_JSON_CLIENT_ID:-[\"X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA\"]}"

if [[ -n "$DB_SHARD_NAME" ]]; then
  MONONOKE_DEFAULT_START_TIMEOUT=60
else
  MONONOKE_DEFAULT_START_TIMEOUT=15
fi

REPOID=0
REPONAME=${REPONAME:-repo}

COMMON_ARGS=(--skip-caching --mysql-master-only --tunables-config "file:$TESTTMP/mononoke_tunables.json")
if [[ -n "$MYSQL_CLIENT" ]]; then
  COMMON_ARGS+=(--use-mysql-client)
fi

TEST_CERTDIR="${HGTEST_CERTDIR:-"$TEST_CERTS"}"

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

function mononoke_address {
  if [[ $LOCALIP == *":"* ]]; then
    # ipv6, surround in brackets
    echo -n "[$LOCALIP]:$MONONOKE_SOCKET"
  else
    echo -n "$LOCALIP:$MONONOKE_SOCKET"
  fi
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
  curl --noproxy localhost --cert "$TEST_CERTDIR/localhost.crt" --cacert "$TEST_CERTDIR/root-ca.crt" --key "$TEST_CERTDIR/localhost.key" "$@"
}

function ssldebuglfssend {
  hg --config extensions.lfs= --config hostsecurity.localhost:verifycertsfile="$TEST_CERTDIR/root-ca.crt" \
    --config auth.lfs.cert="$TEST_CERTDIR/localhost.crt" \
    --config auth.lfs.key="$TEST_CERTDIR/localhost.key" \
    --config auth.lfs.schemes=https \
    --config auth.lfs.prefix=localhost debuglfssend "$@"
}

function mononoke {
  # Ignore specific Python warnings to make tests predictable.
  export MONONOKE_SOCKET
  MONONOKE_SOCKET=$(get_free_socket)

  SCRIBE_LOGS_DIR="$TESTTMP/scribe_logs"
  if [[ ! -d "$SCRIBE_LOGS_DIR" ]]; then
    mkdir "$SCRIBE_LOGS_DIR"
  fi

  PYTHONWARNINGS="ignore:::requests" \
  GLOG_minloglevel=5 "$MONONOKE_SERVER" "$@" \
  --scribe-logging-directory "$TESTTMP/scribe_logs" \
  --ca-pem "$TEST_CERTDIR/root-ca.crt" \
  --private-key "$TEST_CERTDIR/localhost.key" \
  --cert "$TEST_CERTDIR/localhost.crt" \
  --ssl-ticket-seeds "$TEST_CERTDIR/server.pem.seeds" \
  --debug \
  --test-instance \
  --listening-host-port "$(mononoke_address)" \
  --mononoke-config-path "$TESTTMP/mononoke-config" \
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
    --test-instance \
    --local-configerator-path="$TESTTMP/configerator" \
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

function mononoke_x_repo_sync() {
  source_repo_id=$1
  target_repo_id=$2
  shift
  shift
  GLOG_minloglevel=5 "$MONONOKE_X_REPO_SYNC" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --test-instance \
    --local-configerator-path="$TESTTMP/configerator" \
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
  sql_name="${TESTTMP}/hgrepos/repo_lock"

  GLOG_minloglevel=5 "$MONONOKE_HG_SYNC" \
    "${COMMON_ARGS[@]}" \
    --retry-num 1 \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
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

  GLOG_minloglevel=5 "$MONONOKE_BOOKMARKS_FILLER" \
    "${COMMON_ARGS[@]}" \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config \
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
  sqlite3 "$TESTTMP/monsql/sqlite_dbs" < "$TESTTMP"/mutable_counters.sql
}

function init_mutable_counters_sqlite3_db {
  sqlite3 "$TESTTMP/monsql/sqlite_dbs" \
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
    ssh://user@dummy/"$repo" --generate-bundles sync-loop --start-id "$start_id" "$@"
}

function mononoke_admin {
  GLOG_minloglevel=5 "$MONONOKE_ADMIN" \
    "${COMMON_ARGS[@]}" \
    --test-instance \
    --local-configerator-path="$TESTTMP/configerator" \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config "$@"
}

function mononoke_admin_source_target {
  local source_repo_id=$1
  shift
  local target_repo_id=$1
  shift
  GLOG_minloglevel=5 "$MONONOKE_ADMIN" \
    "${COMMON_ARGS[@]}" \
    --test-instance \
    --local-configerator-path="$TESTTMP/configerator" \
    --source-repo-id "$source_repo_id" \
    --target-repo-id "$target_repo_id" \
    --mononoke-config-path "$TESTTMP"/mononoke-config "$@"
}

function mononoke_admin_sourcerepo {
  GLOG_minloglevel=5 "$MONONOKE_ADMIN" \
    "${COMMON_ARGS[@]}" \
    --test-instance \
    --local-configerator-path="$TESTTMP/configerator" \
    --source-repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config "$@"

}
function write_stub_log_entry {
  GLOG_minloglevel=5 "$WRITE_STUB_LOG_ENTRY" \
    "${COMMON_ARGS[@]}" \
    --repo-id $REPOID \
    --mononoke-config-path "$TESTTMP"/mononoke-config --bookmark master_bookmark "$@"
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

  echo "File $file did not contain $count records" >&2
  jq -S . < "$file" >&2
  return 1
}

# Wait until a Mononoke server is available for this repo.
function wait_for_mononoke {
  # MONONOKE_START_TIMEOUT is set in seconds
  # Number of attempts is timeout multiplied by 10, since we
  # sleep every 0.1 seconds.
  local attempts timeout
  timeout="${MONONOKE_START_TIMEOUT:-"$MONONOKE_DEFAULT_START_TIMEOUT"}"
  attempts="$((timeout * 10))"

  SSLCURL="sslcurl https://localhost:$MONONOKE_SOCKET"

  for _ in $(seq 1 $attempts); do
    $SSLCURL 2>&1 | grep -q 'Empty reply' && break
    sleep 0.1
  done

  if ! $SSLCURL 2>&1 | grep -q 'Empty reply'; then
    echo "Mononoke did not start" >&2

    echo ""
    echo "Results of curl invocation"
    $SSLCURL -v

    echo ""
    echo "Log of Mononoke server"
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
[extensions]
commitextras=
[hint]
ack=*
[experimental]
changegroup3=True
[mutation]
record=False
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
  sqlite3 "$TESTTMP/monsql/sqlite_dbs" \
  "insert into bookmarks_update_log \
  (repo_id, name, from_changeset_id, to_changeset_id, reason, timestamp) \
  values(0, 'master_bookmark', NULL, X'04C1EA537B01FFF207445E043E310807F9059572DD3087A0FCE458DEC005E4BD', 'pushrebase', 0)";

  sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from bookmarks_update_log";
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

  if [[ -z "$REPOTYPE" ]]; then
    echo "setup_mononoke_config: REPOTYPE is not set" 2>&1
    exit 1
  fi

  if [[ ! -e "$TESTTMP/mononoke_hgcli" ]]; then
    local priority=""
    if [[ -n "${MONONOKE_HGCLI_PRIORITY:-}" ]]; then
      priority="--priority $MONONOKE_HGCLI_PRIORITY"
    fi

    cat >> "$TESTTMP/mononoke_hgcli" <<EOF
#!/bin/sh
"$MONONOKE_HGCLI" $priority --no-session-output "\$@"
EOF
    chmod a+x "$TESTTMP/mononoke_hgcli"
    MONONOKE_HGCLI="$TESTTMP/mononoke_hgcli"
  fi

  echo "{}" > "$TESTTMP/mononoke_tunables.json"

  cd mononoke-config || exit 1
  mkdir -p common
  touch common/commitsyncmap.toml
  if [[ -n "$SCUBA_CENSORED_LOGGING_PATH" ]]; then
  cat > common/common.toml <<CONFIG
scuba_local_path_censored="$SCUBA_CENSORED_LOGGING_PATH"
CONFIG
  fi
  cat >> common/common.toml <<CONFIG
[[whitelist_entry]]
identity_type = "$ALLOWED_IDENTITY_TYPE"
identity_data = "${OVERRIDE_ALLOWED_IDDATA:-$ALLOWED_IDENTITY_DATA}"
CONFIG

  echo "# Start new config" > common/storage.toml
  setup_mononoke_storage_config "$REPOTYPE" "$blobstorename"

  setup_mononoke_repo_config "$REPONAME" "$blobstorename"
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

  if [[ -n "${MULTIPLEXED:-}" ]]; then
    cat >> common/storage.toml <<CONFIG
$(db_config "$blobstorename")

[$blobstorename.blobstore.multiplexed]
multiplex_id = 1
$(blobstore_db_config)
components = [
CONFIG

    local i
    for ((i=0; i<=MULTIPLEXED; i++)); do
      mkdir -p "$blobstorepath/$i"
      if [[ -n "${PACK_BLOB:-}" && $i -le "$PACK_BLOB" ]]; then
        echo "  { blobstore_id = $i, blobstore = { pack = { blobstore = { $underlyingstorage = { path = \"$blobstorepath/$i\" } } } } }," >> common/storage.toml
      else
        echo "  { blobstore_id = $i, blobstore = { $underlyingstorage = { path = \"$blobstorepath/$i\" } } }," >> common/storage.toml
      fi
    done
    echo ']' >> common/storage.toml
  else
    mkdir -p "$blobstorepath"
    cat >> common/storage.toml <<CONFIG
$(db_config "$blobstorename")

[$blobstorename.blobstore.$underlyingstorage]
path = "$blobstorepath"

CONFIG
  fi
}

function setup_commitsyncmap {
  cp "$TEST_FIXTURES/commitsync/current.toml" "$TESTTMP/mononoke-config/common/commitsyncmap.toml"
}


function setup_configerator_configs {
  export LOADSHED_CONF
  LOADSHED_CONF="$TESTTMP/configerator/scm/mononoke/loadshedding"
  mkdir -p "$LOADSHED_CONF"
  cat >> "$LOADSHED_CONF/limits" <<EOF
{
 "defaults": {
"egress_bytes": 1000000000000,
 "ingress_blobstore_bytes": 1000000000000,
 "total_manifests": 1000000000000,
 "quicksand_manifests": 1000000000,
 "getpack_files": 1000000000,
 "commits": 1000000000
},
"datacenter_prefix_capacity": {
},
"hostprefixes": {
},
"quicksand_multiplier": 1.0,
"rate_limits": {
}
}
EOF

  export PUSHREDIRECT_CONF
  PUSHREDIRECT_CONF="$TESTTMP/configerator/scm/mononoke/pushredirect"
  mkdir -p "$PUSHREDIRECT_CONF"
  cat >> "$PUSHREDIRECT_CONF/enable" <<EOF
{
  "per_repo": {}
}
EOF

  export COMMIT_SYNC_CONF
  COMMIT_SYNC_CONF="$TESTTMP/configerator/scm/mononoke/repos/commitsyncmaps"
  mkdir -p "$COMMIT_SYNC_CONF"
  cp "$TEST_FIXTURES/commitsync/all.json" "$COMMIT_SYNC_CONF/all"
  cp "$TEST_FIXTURES/commitsync/current.json" "$COMMIT_SYNC_CONF/current"
}

function setup_mononoke_repo_config {
  cd "$TESTTMP/mononoke-config" || exit
  local reponame="$1"
  local storageconfig="$2"
  mkdir -p "repos/$reponame"
  mkdir -p "$TESTTMP/monsql"
  mkdir -p "$TESTTMP/$reponame"
  mkdir -p "$TESTTMP/traffic-replay-blobstore"
  cat > "repos/$reponame/server.toml" <<CONFIG
repoid=$REPOID
enabled=${ENABLED:-true}
hash_validation_percentage=100
CONFIG

if [[ -n "${HGSQL_NAME:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
hgsql_name="$HGSQL_NAME"
CONFIG
fi

if [[ -n "${ACL_NAME:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
hipster_acl = "$ACL_NAME"
CONFIG
fi

if [[ -z "${NO_BOOKMARKS_CACHE:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
bookmarks_cache_ttl=2000
CONFIG
fi

if [[ -n "${READ_ONLY_REPO:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
readonly=true
CONFIG
fi

if [[ -n "${SCUBA_LOGGING_PATH:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
scuba_local_path="$SCUBA_LOGGING_PATH"
CONFIG
fi

if [[ -n "${ENFORCE_LFS_ACL_CHECK:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
enforce_lfs_acl_check=true
CONFIG
fi

if [[ -n "${REPO_CLIENT_USE_WARM_BOOKMARKS_CACHE:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
repo_client_use_warm_bookmarks_cache=true
CONFIG
fi

if [[ -n "${WARM_BOOKMARK_CACHE_CHECK_BLOBIMPORT:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
warm_bookmark_cache_check_blobimport=true
CONFIG
fi

# Normally point to common storageconfig, but if none passed, create per-repo
if [[ -z "$storageconfig" ]]; then
  storageconfig="blobstore_$reponame"
  setup_mononoke_storage_config "$REPOTYPE" "$storageconfig"
fi
cat >> "repos/$reponame/server.toml" <<CONFIG
storage_config = "$storageconfig"

CONFIG

if [[ -n "${FILESTORE:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[filestore]
chunk_size = ${FILESTORE_CHUNK_SIZE:-10}
concurrency = 24
CONFIG
fi

if [[ -n "${REDACTION_DISABLED:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
redaction=false
CONFIG
fi

if [[ -n "${LIST_KEYS_PATTERNS_MAX:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
list_keys_patterns_max=$LIST_KEYS_PATTERNS_MAX
CONFIG
fi

if [[ -n "${WIREPROTO_LOGGING_PATH:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[wireproto_logging]
local_path="$WIREPROTO_LOGGING_PATH"
CONFIG

  if [[ -n "${WIREPROTO_LOGGING_BLOBSTORE:-}" ]]; then
    cat >> "repos/$reponame/server.toml" <<CONFIG
storage_config="traffic_replay_blobstore"
remote_arg_size_threshold=0

[storage.traffic_replay_blobstore.metadata.local]
local_db_path="$TESTTMP/monsql"

[storage.traffic_replay_blobstore.blobstore.blob_files]
path = "$WIREPROTO_LOGGING_BLOBSTORE"
CONFIG
  fi
fi
# path = "$TESTTMP/traffic-replay-blobstore"

if [[ -n "${ONLY_FAST_FORWARD_BOOKMARK:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[[bookmarks]]
name="$ONLY_FAST_FORWARD_BOOKMARK"
only_fast_forward=true
CONFIG
fi

if [[ -n "${ONLY_FAST_FORWARD_BOOKMARK_REGEX:-}" ]]; then
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

if [[ -n "${COMMIT_SCRIBE_CATEGORY:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
commit_scribe_category = "$COMMIT_SCRIBE_CATEGORY"
CONFIG
fi

if [[ -n "${ALLOW_CASEFOLDING:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
casefolding_check=false
CONFIG
fi

if [[ -n "${BLOCK_MERGES:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
block_merges=true
CONFIG
fi

if [[ -n "${PUSHREBASE_REWRITE_DATES:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
rewritedates=true
CONFIG
else
  cat >> "repos/$reponame/server.toml" <<CONFIG
rewritedates=false
CONFIG
fi

if [[ -n "${EMIT_OBSMARKERS:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
emit_obsmarkers=true
CONFIG
fi

if [[ -n "${ASSIGN_GLOBALREVS:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
assign_globalrevs=true
CONFIG
fi

if [[ -n "${POPULATE_GIT_MAPPING:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
populate_git_mapping=true
CONFIG
fi

  cat >> "repos/$reponame/server.toml" <<CONFIG

[hook_manager_params]
disable_acl_checker=true
CONFIG

if [[ -n "${ENABLE_PRESERVE_BUNDLE2:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[bundle2_replay_params]
preserve_raw_bundle2 = true
CONFIG
fi

if [[ -n "${DISALLOW_NON_PUSHREBASE:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[push]
pure_push_allowed = false
CONFIG
else
  cat >> "repos/$reponame/server.toml" <<CONFIG
[push]
pure_push_allowed = true
CONFIG
fi

if [[ -n "${COMMIT_SCRIBE_CATEGORY:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
commit_scribe_category = "$COMMIT_SCRIBE_CATEGORY"
CONFIG
fi

if [[ -n "${CACHE_WARMUP_BOOKMARK:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[cache_warmup]
bookmark="$CACHE_WARMUP_BOOKMARK"
CONFIG

  if [[ -n "${CACHE_WARMUP_MICROWAVE:-}" ]]; then
    cat >> "repos/$reponame/server.toml" <<CONFIG
microwave_preload = true
CONFIG
  fi
fi


if [[ -n "${LFS_THRESHOLD:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[lfs]
threshold=$LFS_THRESHOLD
rollout_percentage=${LFS_ROLLOUT_PERCENTAGE:-100}
generate_lfs_blob_in_hg_sync_job=${LFS_BLOB_HG_SYNC_JOB:-true}
CONFIG
fi

write_infinitepush_config "$reponame"

if [[ -n "${ENABLED_DERIVED_DATA:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[derived_data_config]
derived_data_types = $ENABLED_DERIVED_DATA
CONFIG
else
  cat >> "repos/$reponame/server.toml" <<CONFIG
[derived_data_config]
derived_data_types=["blame", "changeset_info", "deleted_manifest", "fastlog", "filenodes", "fsnodes", "unodes", "hgchangesets"]
CONFIG
fi

if [[ -n "${SEGMENTED_CHANGELOG_ENABLE:-}" ]]; then
  cat >> "repos/$reponame/server.toml" <<CONFIG
[segmented_changelog_config]
enabled=true
CONFIG
  if [[ -n "${SEGMENTED_CHANGELOG_ON_DEMAND_UPDATE:-}" ]]; then
    cat >> "repos/$reponame/server.toml" <<CONFIG
update_algorithm="ondemand"
CONFIG
  fi
fi
}

function write_infinitepush_config {
  local reponame="$1"
  if [[ -n "${INFINITEPUSH_ALLOW_WRITES:-}" ]] || \
     [[ -n "${INFINITEPUSH_NAMESPACE_REGEX:-}" ]] || \
     [[ -n "${INFINITEPUSH_HYDRATE_GETBUNDLE_RESPONSE:-}" ]] || \
     [[ -n "${INFINITEPUSH_POPULATE_REVERSE_FILLER_QUEUE:-}" ]];
  then
    namespace=""
    if [[ -n "${INFINITEPUSH_NAMESPACE_REGEX:-}" ]]; then
      namespace="namespace_pattern=\"$INFINITEPUSH_NAMESPACE_REGEX\""
    fi

    cat >> "repos/$reponame/server.toml" <<CONFIG
[infinitepush]
allow_writes = ${INFINITEPUSH_ALLOW_WRITES:-true}
hydrate_getbundle_response = ${INFINITEPUSH_HYDRATE_GETBUNDLE_RESPONSE:-false}
populate_reverse_filler_queue = ${INFINITEPUSH_POPULATE_REVERSE_FILLER_QUEUE:-false}
${namespace}
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

  (
    cat <<CONFIG
[[bookmarks.hooks]]
hook_name="$hook_name"
[[hooks]]
name="$hook_name"
CONFIG
    [ -n "$EXTRA_CONFIG_DESCRIPTOR" ] && cat "$EXTRA_CONFIG_DESCRIPTOR"
  ) >> "repos/$REPONAME/server.toml"
}

function backfill_git_mapping {
  GLOG_minloglevel=5 "$MONONOKE_BACKFILL_GIT_MAPPING" --repo_id "$REPOID" \
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
  mkdir -p "$output"
  $MONONOKE_BLOBIMPORT --repo-id $REPOID \
     --mononoke-config-path "$TESTTMP/mononoke-config" \
     "$input" "${COMMON_ARGS[@]}" "$@" > "$TESTTMP/blobimport.out" 2>&1
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

function lfs_import {
  GLOG_minloglevel=5 "$MONONOKE_LFS_IMPORT" --repo-id "$REPOID" \
  --mononoke-config-path "$TESTTMP/mononoke-config" "${COMMON_ARGS[@]}" "$@"
}

function manual_scrub {
  GLOG_minloglevel=5 "$MONONOKE_MANUAL_SCRUB" \
  --mononoke-config-path "$TESTTMP/mononoke-config" "${COMMON_ARGS[@]}" "$@"
}

function s_client {
    /usr/local/fbcode/platform007/bin/openssl s_client \
        -connect "$(mononoke_address)" \
        -CAfile "${TEST_CERTDIR}/root-ca.crt" \
        -cert "${TEST_CERTDIR}/localhost.crt" \
        -key "${TEST_CERTDIR}/localhost.key" \
        -ign_eof "$@"
}

function start_and_wait_for_scs_server {
  export SCS_PORT
  SCS_PORT=$(get_free_socket)
  GLOG_minloglevel=5 "$SCS_SERVER" "$@" \
    -p "$SCS_PORT" \
    --log-level DEBUG \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --test-instance \
    --local-configerator-path="$TESTTMP/configerator" \
    "${COMMON_ARGS[@]}" >> "$TESTTMP/scs_server.out" 2>&1 &
  export SCS_SERVER_PID=$!
  echo "$SCS_SERVER_PID" >> "$DAEMON_PIDS"

  # Wait until a SCS server is available
  # MONONOKE_START_TIMEOUT is set in seconds
  # Number of attempts is timeout multiplied by 10, since we
  # sleep every 0.1 seconds.
  local attempts timeout
  timeout="${MONONOKE_START_TIMEOUT:-"$MONONOKE_DEFAULT_START_TIMEOUT"}"
  attempts="$((timeout * 10))"

  CHECK_SSL="/usr/local/fbcode/platform007/bin/openssl s_client -connect localhost:$SCS_PORT"

  for _ in $(seq 1 $attempts); do
    $CHECK_SSL 2>&1  </dev/null | grep -q 'DONE' && break
    sleep 0.1
  done

  if ! $CHECK_SSL 2>&1 </dev/null | grep -q 'DONE'; then
    echo "SCS server did not start" >&2
    cat "$TESTTMP/scs_server.out"
    exit 1
  fi
}

function scsc {
  GLOG_minloglevel=5 "$SCS_CLIENT" --host "localhost:$SCS_PORT" "$@"
}

function start_edenapi_server {
  local port log attempts timeout
  port=$(get_free_socket)
  log="$TESTTMP/edenapi_server.out"

  # Start the EdenAPI server, using test TLS credentials.
  # This means that tests must use TLS to connect to the
  # server. The `sslcurl` function should help with this.
  GLOG_minloglevel=5 "$EDENAPI_SERVER" "$@" \
    --debug \
    --listen-host "$LOCALIP" \
    --listen-port "$port" \
    --mononoke-config-path "$TESTTMP/mononoke-config" \
    --test-instance \
    --local-configerator-path="$TESTTMP/configerator" \
    --tls-ca "$TEST_CERTDIR/root-ca.crt" \
    --tls-private-key "$TEST_CERTDIR/localhost.key" \
    --tls-certificate "$TEST_CERTDIR/localhost.crt" \
    --tls-ticket-seeds "$TEST_CERTDIR/server.pem.seeds" \
    --trusted-proxy-identity USER:myusername0 \
    "${COMMON_ARGS[@]}" >> "$log" 2>&1 &

  # Record the PID of the spawned process so the test framework
  # can kill it later during cleanup.
  echo "$!" >> "$DAEMON_PIDS"

  # Export the URI that tests will use to connect to the server.
  export EDENAPI_URI="https://localhost:$port"

  # Wait for the server to start listening for HTTP requests.
  timeout="${MONONOKE_START_TIMEOUT:-"$MONONOKE_DEFAULT_START_TIMEOUT"}"
  attempts="$((timeout * 10))"
  for _ in $(seq 1 $attempts); do
    if sslcurl -q "$EDENAPI_URI/health_check" > /dev/null 2>&1; then
      truncate -s 0 "$log"
      return 0
    fi
    sleep 0.1
  done

  echo "EdenAPI server failed to start" >&2
  cat "$log" >&2
  return 1
}

function edenapi_make_req {
  "$EDENAPI_MAKE_REQ" "$@"
}

function edenapi_read_res {
  "$EDENAPI_READ_RES" "$@"
}

function lfs_server {
  local port uri log opts args proto poll lfs_server_pid
  port="$(get_free_socket)"
  log="${TESTTMP}/lfs_server.${port}"
  proto="http"
  poll="curl"

  opts=(
    "${COMMON_ARGS[@]}"
    --mononoke-config-path "$TESTTMP/mononoke-config"
    --listen-host "$LOCALIP"
    --listen-port "$port"
    --test-friendly-logging
  )
  args=()

  while [[ "$#" -gt 0 ]]; do
    if [[ "$1" = "--upstream" ]]; then
      shift
      args=("${args[@]}" "$1")
      shift
    elif [[ "$1" = "--live-config" ]]; then
      opts=("${opts[@]}" "$1" "$2" "--live-config-fetch-interval" "1")
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
    elif [[ "$1" = "--always-wait-for-upstream" ]]; then
      opts=("${opts[@]}" "$1")
      shift
    elif
      [[ "$1" = "--allowed-test-identity" ]] ||
      [[ "$1" = "--scuba-log-file" ]] ||
      [[ "$1" = "--trusted-proxy-identity" ]] ||
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

  uri="${proto}://localhost:${port}"
  echo "$uri"

  GLOG_minloglevel=5 "$LFS_SERVER" \
    "${opts[@]}" "$uri" "${args[@]}" >> "$log" 2>&1 &

  lfs_server_pid="$!"
  echo "$lfs_server_pid" >> "$DAEMON_PIDS"

  for _ in $(seq 1 200); do
    if "$poll" "${uri}/health_check" >/dev/null 2>&1; then
      truncate -s 0 "$log"
      return 0
    fi

    sleep 0.1
  done

  echo "lfs_server did not start:" >&2
  cat "$log" >&2
  return 1
}

function extract_json_error {
  input=$(< /dev/stdin)
  echo "$input" | head -1 | jq -r '.message'
  echo "$input" | tail -n +2
}

# Run an hg binary configured with the settings required to talk to Mononoke.
function hgmn {
  hg --config ui.ssh="$DUMMYSSH" --config paths.default=ssh://user@dummy/$REPONAME --config ui.remotecmd="$MONONOKE_HGCLI" "$@"
}

function hgmn_local {
  hg --config ui.ssh="${TEST_FIXTURES}/nossh.sh" "$@"
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
  hgmn clone -q --shallow --config remotefilelog.reponame=master "$@" --config extensions.treemanifest= --config treemanifest.treeonly=True --config extensions.lz4revlog=
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
commitextras=
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
[mutation]
record=False
EOF
}

# Does all the setup necessary for hook tests
function hook_test_setup() {
  # shellcheck source=fbcode/eden/mononoke/tests/integration/library.sh
  . "${TEST_FIXTURES}/library.sh"

  setup_mononoke_config
  cd "$TESTTMP/mononoke-config" || exit 1

  HOOKBOOKMARK="${HOOKBOOKMARK:-master_bookmark}"
  cat >> "repos/$REPONAME/server.toml" <<CONFIG
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
url=$1
threshold=$2
backofftimes=0
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

function call_with_certs() {
  REPLAY_CA_PEM="$TEST_CERTDIR/root-ca.crt" \
  THRIFT_TLS_CL_CERT_PATH="$TEST_CERTDIR/localhost.crt" \
  THRIFT_TLS_CL_KEY_PATH="$TEST_CERTDIR/localhost.key" \
  GLOG_minloglevel=5 "$@"
}

function traffic_replay() {
  call_with_certs "$TRAFFIC_REPLAY" \
    --loglevel warn \
    --testrun \
    --hgcli "$MONONOKE_HGCLI" \
    --mononoke-address "$(mononoke_address)" \
    --mononoke-server-common-name localhost
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

function create_replaybookmarks_table() {
  if [[ -n "$DB_SHARD_NAME" ]]; then
    # We don't need to do anything: the MySQL setup creates this for us.
    true
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

function add_synced_commit_mapping_entry() {
  local small_repo_id large_repo_id small_bcs_id large_bcs_id
  small_repo_id="$1"
  small_bcs_id="$2"
  large_repo_id="$3"
  large_bcs_id="$4"
  sqlite3 "$TESTTMP/monsql/sqlite_dbs" <<EOF
    INSERT INTO synced_commit_mapping (small_repo_id, small_bcs_id, large_repo_id, large_bcs_id)
    VALUES ('$small_repo_id', X'$small_bcs_id', '$large_repo_id', X'$large_bcs_id');
EOF
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
  blobimport repo-hg/.hg repo
}

function default_setup() {
  default_setup_blobimport "$BLOB_TYPE"
  echo "starting Mononoke"
  mononoke "$@"
  wait_for_mononoke "$TESTTMP/repo"

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
  GIT_COMMITTER_DATE="$date" \
  GIT_COMMITTER_NAME="$name" \
  GIT_COMMITTER_EMAIL="$email" \
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
fi

function regenerate_hg_filenodes() {
  "$MONONOKE_REGENERATE_HG_FILENODES" \
    "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    --i-know-what-i-am-doing \
    "$@"
}

function fastreplay() {
  "$MONONOKE_FASTREPLAY" \
    "${COMMON_ARGS[@]}" \
    --no-skiplist \
    --no-cache-warmup \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function microwave_builder() {
  "$MONONOKE_MICROWAVE_BUILDER" \
    "${COMMON_ARGS[@]}" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function unbundle_replay() {
  "$MONONOKE_UNBUNDLE_REPLAY" \
    "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}

function backfill_derived_data() {
  "$MONONOKE_BACKFILL_DERIVED_DATA" \
    "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
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

function commitcloud_fill_one() {
  # Note: do not use `call_with_certs` here or
  # put cert paths into env vars, as this causes
  # the binary to lose access to `getdb.sh`-acquired
  # database
  local CERTDIR="${HGTEST_CERTDIR:-"$TEST_CERTS"}"
  "$MONONOKE_COMMITCLOUD_FILLONE" \
    hg-to-mononoke \
    --hgcli "$MONONOKE_HGCLI" \
    --mononoke-address "$(mononoke_address)" \
    --mononoke-server-common-name localhost \
    --cert-override "$CERTDIR/localhost.crt" \
    --private-key-override "$CERTDIR/localhost.key" \
    --ca-pem-override "$CERTDIR/root-ca.crt" \
    --reponame $REPONAME \
    --rebundler-path "$MONONOKE_COMMITCLOUD_INFINITEPUSHREBUNDLE/infinitepushrebundle.py" \
    --debug "$@"
}

function commitcloud_reverse_fill_one() {
  "$MONONOKE_COMMITCLOUD_FILLONE" \
    mononoke-to-hg \
    --reponame $REPONAME \
    --rebundler-path "$MONONOKE_COMMITCLOUD_INFINITEPUSHREBUNDLE/infinitepushrebundle.py" \
    --debug "$@"
}

function commitcloud_reversefiller_iteration() {
  local default_sqlite_db="$TESTTMP/monsql/sqlite_dbs"
  local sqlitedb="${REVERSEFILLER_SQLITE_DB:-"$default_sqlite_db"}"
  "$MONONOKE_COMMITCLOUD_REVERSEFILLER" \
    --num-workers 1 \
    --identity "testfiller" \
    --reponame "$REPONAME" \
    --rebundler-path "$MONONOKE_COMMITCLOUD_INFINITEPUSHREBUNDLE/infinitepushrebundle.py" \
    --stop-after-n-iterations=10 \
    --with-sqlite-db="$sqlitedb" \
    --debug "$@"
}

function commitcloud_forwardfiller_iteration() {
  # Note: do not use `call_with_certs` here or
  # put cert paths into env vars, as this causes
  # the binary to lose access to `getdb.sh`-acquired
  # database
  local CERTDIR="${HGTEST_CERTDIR:-"$TEST_CERTS"}"
  "$MONONOKE_COMMITCLOUD_FORWARDFILLER" \
    --num-workers 1 \
    --hgcli "$MONONOKE_HGCLI" \
    --mononoke-address "$(mononoke_address)" \
    --mononoke-server-common-name localhost \
    --cert-override "$CERTDIR/localhost.crt" \
    --private-key-override "$CERTDIR/localhost.key" \
    --ca-pem-override "$CERTDIR/root-ca.crt" \
    --identity "testfiller" \
    --reponame "$REPONAME" \
    --rebundler-path "$MONONOKE_COMMITCLOUD_INFINITEPUSHREBUNDLE/infinitepushrebundle.py" \
    --stop-after-n-iterations=10 \
    --debug "$@"
}

function repo_import() {
  log="$TESTTMP/repo_import.out"

  "$MONONOKE_REPO_IMPORT" \
    "${COMMON_ARGS[@]}" \
    --repo-id "$REPOID" \
    --mononoke-config-path "${TESTTMP}/mononoke-config" \
    "$@"
}
