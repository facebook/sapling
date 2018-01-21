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
