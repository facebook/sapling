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

# return random value from [1, max_value]
function random_int() {
  max_value=$1

  VAL=$RANDOM
  (( VAL %= $max_value ))
  (( VAL += 1 ))

  echo $VAL
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
  -P "$TESTTMP/mononoke-config" \
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
    setup_mononoke_config "$@"
    setup_common_hg_configs
}

function setup_mononoke_config {
  cd "$TESTTMP" || exit
  mkdir mononoke-config
  REPOTYPE="blob:rocks"
  if [[ $# -gt 0 ]]; then
    REPOTYPE="$1"
  fi

  cd mononoke-config
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

function register_hook {
  hook_name="$1"
  path="$2"
  hook_type="$3"

  shift 3
  BYPASS=""
  if [[ $# -gt 0 ]]; then
    BYPASS="$1"
  fi

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
  input="$1"
  output="$2"
  shift 2
  mkdir -p "$output"
  $MONONOKE_BLOBIMPORT --repo_id 0 \
     --mononoke-config-path "$TESTTMP/mononoke-config" \
     "$input" --do-not-init-cachelib "$@" >> "$TESTTMP/blobimport.out" 2>&1
  BLOBIMPORT_RC="$?"
  if [[ $BLOBIMPORT_RC -ne 0 ]]; then
    cat "$TESTTMP/blobimport.out"
    # set exit code, otherwise previous cat sets it to 0
    return "$BLOBIMPORT_RC"
  fi
}

function bonsai_verify {
  GLOG_minloglevel=2 $MONONOKE_BONSAI_VERIFY --repo_id 0 \
  --mononoke-config-path "$TESTTMP/mononoke-config" "$@"
}

function setup_no_ssl_apiserver {
  APISERVER_PORT=$(get_free_socket)
  no_ssl_apiserver --http-host "127.0.0.1" --http-port "$APISERVER_PORT"
  wait_for_apiserver --no-ssl
}


function apiserver {
  $MONONOKE_APISERVER "$@" --mononoke-config-path "$TESTTMP/mononoke-config" \
    --ssl-ca "$TESTDIR/testcert.crt" \
    --ssl-private-key "$TESTDIR/testcert.key" \
    --ssl-certificate "$TESTDIR/testcert.crt" \
    --ssl-ticket-seeds "$TESTDIR/server.pem.seeds" \
    --do-not-init-cachelib >> "$TESTTMP/apiserver.out" 2>&1 &
  export APISERVER_PID=$!
  echo "$APISERVER_PID" >> "$DAEMON_PIDS"
}

function no_ssl_apiserver {
  $MONONOKE_APISERVER "$@" \
   --mononoke-config-path "$TESTTMP/mononoke-config" >> "$TESTTMP/apiserver.out" 2>&1 &
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

  setup_mononoke_config
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

  if [[ ! -z "$HOOK_FILE" ]] ; then
    mkdir -p common/hooks
    cp "$HOOK_FILE" common/hooks/"$HOOK_NAME".lua
    register_hook "$HOOK_NAME" common/hooks/"$HOOK_NAME".lua "$HOOK_TYPE" "$BYPASS"
  else
    register_hook "$HOOK_NAME" "" "$HOOK_TYPE" "$BYPASS"
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

  hg bookmark master_bookmark -r tip

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
     --mononoke-config-path "$TESTTMP/mononoke-config" \
     --mode "$mode" "$@"
}
