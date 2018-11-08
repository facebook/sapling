#!/bin/bash
# Library routines and initial setup for Mononoke-related tests.

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

function sslcurl {
  curl --cert "$TESTDIR/testcert.crt" --cacert "$TESTDIR/testcert.crt" --key "$TESTDIR/testcert.key" "$@"
}

function mononoke {
  export MONONOKE_SOCKET
  MONONOKE_SOCKET=$(get_free_socket)
  "$MONONOKE_SERVER" "$@" --ca-pem "$TESTDIR/testcert.crt" \
  --private-key "$TESTDIR/testcert.key" \
  --cert "$TESTDIR/testcert.crt" \
  --debug \
  --listening-host-port 127.0.0.1:"$MONONOKE_SOCKET" \
  -P "$TESTTMP/mononoke-config-rocks" \
   --configrepo_book local_master \
   --do-not-init-cachelib >> "$TESTTMP/mononoke.out" 2>&1 &
  echo $! >> "$DAEMON_PIDS"
}

# Wait until a Mononoke server is available for this repo.
function wait_for_mononoke {
  local attempts=150

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
    setup_config_repo "$@"
    setup_common_hg_configs
}

function setup_config_repo {
  setup_hg_config_repo "$@"
  commit_and_blobimport_config_repo
}

function setup_hg_config_repo {
  hg init mononoke-config
  cd mononoke-config || exit
  cat >> .hg/hgrc <<EOF
[extensions]
treemanifest=
remotefilelog=
[treemanifest]
server=True
[remotefilelog]
server=True
shallowtrees=True
EOF

  REPOTYPE="blob:rocks"
  if [[ $# -gt 0 ]]; then
    REPOTYPE="$1"
  fi

  mkdir -p repos/repo
  cat > repos/repo/server.toml <<CONFIG
path="$TESTTMP/repo"
repotype="$REPOTYPE"
repoid=0
enabled=true
CONFIG

if [[ -v READ_ONLY_REPO ]]; then
  cat >> repos/repo/server.toml <<CONFIG
readonly=true
CONFIG
fi

  cat >> repos/repo/server.toml <<CONFIG
[pushrebase]
rewritedates=false
CONFIG

if [[ -v CACHE_WARMUP_BOOKMARK ]]; then
  cat >> repos/repo/server.toml <<CONFIG
[cache_warmup]
bookmark="$CACHE_WARMUP_BOOKMARK"
CONFIG
fi

if [[ -v LFS_THRESHOLD ]]; then
  cat >> repos/repo/server.toml <<CONFIG
[lfs]
threshold=$LFS_THRESHOLD
CONFIG
fi

  mkdir -p repos/disabled_repo
  cat > repos/disabled_repo/server.toml <<CONFIG
path="$TESTTMP/disabled_repo"
repotype="$REPOTYPE"
repoid=2
enabled=false
CONFIG
}

function commit_and_blobimport_config_repo {
  hg ci -Aqma
  hg backfilltree
  hg book local_master
  cd ..

  # We need to have a RocksDb version of config repo
  mkdir mononoke-config-rocks
  blobimport rocksdb mononoke-config/.hg mononoke-config-rocks
}

function register_hook {
  path="$1"
  hook_type="$2"

  shift 2
  BYPASS=""
  if [[ $# -gt 0 ]]; then
    BYPASS="$1"
  fi

  hook_basename=${path##*/}
  hook_name="${hook_basename%.lua}"
  cat >> repos/repo/server.toml <<CONFIG
[[bookmarks.hooks]]
hook_name="$hook_name"
[[hooks]]
name="$hook_name"
path="$path"
hook_type="$hook_type"
$BYPASS
CONFIG
}

function blobimport {
  blobstore="$1"
  input="$2"
  output="$3"
  shift 3
  mkdir -p "$output"
  $MONONOKE_BLOBIMPORT --repo_id 0 \
     --blobstore "$blobstore" "$input" \
     --data-dir "$output" --do-not-init-cachelib "$@" >> "$TESTTMP/blobimport.out" 2>&1
  BLOBIMPORT_RC="$?"
  if [[ $BLOBIMPORT_RC -ne 0 ]]; then
    cat "$TESTTMP/blobimport.out"
    # set exit code, otherwise previous cat sets it to 0
    return "$BLOBIMPORT_RC"
  fi
}

function bonsai_verify {
  repo="$1"
  shift 1
  $MONONOKE_BONSAI_VERIFY --repo_id 0 --blobstore rocksdb --data-dir "$repo" "$@"
}

function apiserver {
  $MONONOKE_APISERVER "$@" --config-path "$TESTTMP/mononoke-config-rocks" \
    --config-bookmark "local_master" \
    --ssl-ca "$TESTDIR/testcert.crt" \
    --ssl-private-key "$TESTDIR/testcert.key" \
    --ssl-certificate "$TESTDIR/testcert.crt" \
    --do-not-init-cachelib >> "$TESTTMP/apiserver.out" 2>&1 &
  echo $! >> "$DAEMON_PIDS"
}

function no_ssl_apiserver {
  $MONONOKE_APISERVER "$@" \
   --config-path "$TESTTMP/mononoke-config-rocks" \
  --config-bookmark "local_master" >> "$TESTTMP/apiserver.out" 2>&1 &
  echo $! >> "$DAEMON_PIDS"
}

function wait_for_apiserver {
  for _ in $(seq 1 200); do
    PORT=$(grep "Listening to" < "$TESTTMP/apiserver.out" | grep -Pzo "(\\d+)\$") && break
    sleep 0.1
  done

  if [[ -z "$PORT" ]]; then
    echo "error: Mononoke API Server is not started"
    cat "$TESTTMP/apiserver.out"
    exit 1
  fi

  export APISERVER
  APISERVER="https://localhost:$PORT"
  if [[ ($# -eq 1 && "$1" == "--no-ssl") ]]; then
    APISERVER="http://localhost:$PORT"
  fi
}

function extract_json_error {
  input=$(cat)
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
remotefilelog=
smartlog=
[treemanifest]
server=True
sendtrees=True
[remotefilelog]
reponame=$1
cachepath=$TESTTMP/cachepath
server=True
shallowtrees=True
EOF
}

function hgclone_treemanifest() {
  hg clone -q --shallow --config remotefilelog.reponame=master "$@"
  cat >> "$2"/.hg/hgrc <<EOF
[extensions]
treemanifest=
remotefilelog=
fastmanifest=
smartlog=
[treemanifest]
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
[treemanifest]
server=False
treeonly=True
[remotefilelog]
server=False
reponame=repo
EOF
}

# Does all the setup necessary for hook tests
function hook_test_setup() {
  . "$TESTDIR"/library.sh

  setup_hg_config_repo
  cd "$TESTTMP/mononoke-config" || exit 1

  cat >> repos/repo/server.toml <<CONFIG
[[bookmarks]]
name="master_bookmark"
CONFIG

  HOOK_FILE="$1"
  HOOK_NAME="$2"
  HOOK_TYPE="$3"
  shift 3
  BYPASS=""
  if [[ $# -gt 0 ]]; then
    BYPASS="$1"
  fi

  mkdir -p common/hooks
  cp "$HOOK_FILE" common/hooks/"$HOOK_NAME".lua
  register_hook common/hooks/"$HOOK_NAME".lua "$HOOK_TYPE" "$BYPASS"

  commit_and_blobimport_config_repo
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

  hg bookmark master_bookmark -r tip

  cd ..
  blobimport rocksdb repo-hg/.hg repo

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
  blobstore=$1
  repo_folder=$2
  mode=$3
  shift 3
  $MONONOKE_ALIAS_VERIFY --repo_id 0 --blobstore "$blobstore" --data-dir "$repo_folder" --mode "$mode" "$@"
}
