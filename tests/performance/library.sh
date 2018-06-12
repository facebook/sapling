#!/bin/bash

source ../integration/library.sh

function kill_all_children {
  mapfile -t args < <(jobs -p)
  if [[ ${#args[@]} -ne 0 ]]; then
    kill "${args[@]}" || true
  fi
}

function common_setup {
  trap kill_all_children EXIT

  export RUST_BACKTRACE=1

  export HGRCPATH="$HOME/.hgrc"
  INSTANCE="$(id -un)-$(date +%s)"
  REPO_PATH="/tmp/mononoke-perf-$INSTANCE"
  mkdir -p "$REPO_PATH"
  export TESTTMP="$REPO_PATH"
  export DAEMON_PIDS="$REPO_PATH/daemon.pids"
}

function build_tools {
  echo "Building dummy SSH"
  DUMMYSSH="$(buck root)/$(buck build @mode/opt '//scm/mononoke/tests/integration:dummyssh' --show-output | cut -d\  -f2)"
  export DUMMYSSH
  echo "Building Mononoke hgcli"
  MONONOKE_HGCLI="$(buck root)/$(buck build @mode/opt '//scm/mononoke/hgcli:hgcli' --show-output | cut -d\  -f2)"
  echo "Building Mononoke blobimport"
  MONONOKE_BLOBIMPORT="$(buck root)/$(buck build @mode/opt '//scm/mononoke:blobimport' --show-output | cut -d\  -f2)"
  export MONONOKE_BLOBIMPORT
  echo "Building Mononoke server"
  MONONOKE_SERVER="$(buck root)/$(buck build @mode/opt '//scm/mononoke:mononoke' --show-output | cut -d\  -f2)"
  export MONONOKE_SERVER
  echo "Building repository synthesizer"
  REPOSYNTHESIZER="$(buck root)/$(buck build @mode/opt '//scm/mononoke/facebook/reposynthesizer:reposynthesizer' --show-output | cut -d\  -f2)"
  export REPOSYNTHESIZER
  echo
}

function hginit_treemanifest() {
  hg init "$@"
  cat >> "$1"/.hg/hgrc <<EOF
[extensions]
treemanifest=
remotefilelog=
fastmanifest=
[treemanifest]
server=False
treeonly=True
sendtrees=True
[remotefilelog]
server=False
reponame=$1
cachepath=$TESTTMP/cachepath
shallowtrees=True
EOF
}

function setup_manifold_config_repo {
  local repos_path=$1
  local config_repo="$repos_path/mononoke-config"
  local prefix=$2
  local scuba_table="mononoke_test_perf"

  cd "$repos_path" || exit
  hg init "$config_repo"
  cd "$config_repo" || exit
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

  mkdir repos
  cat > repos/repo <<CONFIG
path="$repos_path/repo"
repotype="blob:testmanifold"
manifold_bucket="mononoke"
manifold_prefix="$prefix-"
scuba_table="$scuba_table"
repoid=0
CONFIG
  hg add -q repos
  hg ci -ma
  hg backfilltree
  mkdir "$config_repo-rocks"

  $MONONOKE_BLOBIMPORT --repo_id 0 --blobstore rocksdb "$config_repo"/.hg "$config_repo"-rocks >> "$REPO_PATH/blobimport.out" 2>&1

  mkdir -p "$repos_path/repo/.hg"

  echo "Scuba table is $scuba_table and repo in that table is $repos_path/repo"
}

function setup_testdelay_config_repo {
  local repos_path=$1
  local config_repo="$repos_path/mononoke-config"
  local prefix=$2
  local scuba_table="mononoke_test_perf"

  cd "$repos_path" || exit
  hg init "$config_repo"
  cd "$config_repo" || exit
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

# https://fburl.com/ods/icc3brsq gives raw Manifold latency for this purpose
  mkdir repos
  cat > repos/repo <<CONFIG
path="$repos_path/repo"
repotype="blob:testdelay"
scuba_table="$scuba_table"
delay_mean=150000
delay_stddev=1200
repoid=0
CONFIG
  hg add -q repos
  hg ci -ma
  hg bookmark test-config
  hg backfilltree
  mkdir "$config_repo-rocks"

  $MONONOKE_BLOBIMPORT --repo_id 0 --blobstore rocksdb "$config_repo"/.hg "$config_repo"-rocks >> "$REPO_PATH/blobimport.out" 2>&1

  mkdir -p "$repos_path/repo/.hg"

  echo "Scuba table is $scuba_table and repo in that table is $repos_path/repo"
}

function run_mononoke {
  mononoke
  echo "Mononoke output at $REPO_PATH/mononoke.out"
  wait_for_mononoke "$REPO_PATH/repo"
  echo

  echo "Mononoke running"
}

function cleanup {
  echo "Waiting 10 seconds to ensure that Mononoke has written out logs"
  sleep 10
  echo

  echo "Killing Mononoke"
  kill_all_children
  echo
  rm -fr "$REPO_PATH"
  echo "Cleaned up"

  wait || true
  echo "Mononoke terminated"
}
