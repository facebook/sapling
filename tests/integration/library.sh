#!/bin/bash
# Library routines and initial setup for Mononoke-related tests.

function mononoke {
  $MONONOKE_SERVER "$@" >> "$TESTTMP/mononoke.out" 2>&1 &
  echo $! >> "$DAEMON_PIDS"
}

# Wait until a Mononoke server is available for this repo.
function wait_for_mononoke {
  local socket="$1/.hg/mononoke.sock"
  local attempts=50
  until [[ -S $socket || $attempts -le 0 ]]; do
    attempts=$((attempts - 1))
    sleep 0.1
  done
}

function setup_config_repo {
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

  mkdir repos
  cat > repos/repo <<CONFIG
path="$TESTTMP/repo"
repotype="blob:files"
CONFIG
  hg add -q repos
  hg ci -ma
  hg bookmark test-config
  hg backfilltree
  cd ..
}
function blobimport {
  $MONONOKE_BLOBIMPORT "$@"
}

function edenserver {
  $MONONOKE_EDEN_SERVER "$@" >> "$TESTTMP/edenserver.out" 2>&1 &
  echo $! >> "$DAEMON_PIDS"
}

# Run an hg binary configured with the settings required to talk to Mononoke.
function hgmn {
  hg --config ui.ssh="$DUMMYSSH" --config ui.remotecmd="$MONONOKE_HGCLI" "$@"
}

hgcloneshallow() {
  local dest
  orig=$1
  shift
  dest=$1
  shift
  hg clone --shallow --config remotefilelog.reponame=master "$orig" "$dest" "$@"
  cat >> "$dest"/.hg/hgrc <<EOF
[remotefilelog]
reponame=master
datapackversion=1
[phases]
publish=False
EOF
}
